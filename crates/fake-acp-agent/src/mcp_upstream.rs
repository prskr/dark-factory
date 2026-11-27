//! A fake third-party MCP server for the end-to-end cluster (`--serve-fake-mcp`): Streamable HTTP
//! without sessions, answering with JSON. Every request needs `Authorization: Bearer <token>`.
//! The tool `whoami` echoes the credential it received, so a test sees that the gateway redacts it.
//!
//! The path says which registered server a request is for (`/mcp/<name>`): the tool of `/mcp/<name>`
//! answers `<name> reached` plus the credential line, so a call that must not reach a server
//! never shows that text.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

const MAX_HEAD_BYTES: usize = 64 * 1024;

/// Serves on `address` until the process ends.
pub async fn serve(address: &str, token: String) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("bind {address}"))?;
    serve_on(listener, token).await
}

/// Serves on `listener`. The connection tasks belong to the server: they end with it.
async fn serve_on(listener: TcpListener, token: String) -> Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _peer) = accepted.context("accept")?;
                let token = token.clone();
                connections.spawn(async move {
                    if let Err(error) = answer(stream, &token).await {
                        log(&error);
                    }
                });
            }
            // Finished connections are collected, so the set does not grow.
            Some(_finished) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

#[expect(clippy::print_stderr, reason = "the fake server has no logger")]
fn log(error: &anyhow::Error) {
    eprintln!("fake MCP server: {error:#}");
}

async fn answer(mut stream: TcpStream, token: &str) -> Result<()> {
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
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let authorized = lower
        .lines()
        .any(|line| line == format!("authorization: bearer {}", token.to_ascii_lowercase()));
    let response = if !head.starts_with("POST ") {
        plain("405 Method Not Allowed", "")
    } else if !authorized {
        plain("401 Unauthorized", r#"{"error":"unauthorized"}"#)
    } else {
        let message: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        match reply(path, token, &message) {
            Some(value) => plain("200 OK", &value.to_string()),
            None => plain("202 Accepted", ""),
        }
    };
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await.ok();
    Ok(())
}

/// The JSON-RPC answer to `message`, or `None` for a notification.
fn reply(path: &str, token: &str, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let server = path.rsplit('/').next().unwrap_or_default();
    let result = match message.get("method").and_then(Value::as_str) {
        Some("initialize") => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "fake-mcp", "version": "1" },
        }),
        Some("tools/list") => json!({ "tools": [{
            "name": "whoami",
            "title": "Who am I",
            "description": "Says which server answered and with which credential.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true },
        }] }),
        Some("tools/call") => json!({ "content": [{
            "type": "text",
            "text": format!("{server} reached; credential line: Authorization: Bearer {token}"),
        }] }),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "Method not found" },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    async fn post(address: &str, token: &str, path: &str, body: &Value) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let body = body.to_string();
        let request = format!(
            "POST {path} HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {token}\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn connections_end_with_the_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(serve_on(listener, "secret".into()));
        // A request whose head never ends keeps its connection task waiting.
        let mut stream = TcpStream::connect(&address).await.unwrap();
        stream
            .write_all(b"POST /mcp HTTP/1.1\r\nhost: x\r\n")
            .await
            .unwrap();
        let mut buffer = [0_u8; 1];
        let held = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            stream.read(&mut buffer),
        )
        .await;
        assert!(
            held.is_err(),
            "the connection is held open while the server runs"
        );

        server.abort();

        let ended =
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut buffer))
                .await
                .expect("the connection was not closed when the server stopped");
        assert!(matches!(ended, Ok(0) | Err(_)));
    }

    #[tokio::test]
    async fn answers_the_handshake_lists_and_calls_only_with_the_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(serve_on(listener, "secret".into()));

        let init = post(
            &address,
            "secret",
            "/mcp/docs",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
        )
        .await;
        let notified = post(
            &address,
            "secret",
            "/mcp/docs",
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        let list = post(
            &address,
            "secret",
            "/mcp/docs",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .await;
        let call = post(&address, "secret", "/mcp/docs", &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "whoami"}})).await;
        let refused = post(
            &address,
            "wrong",
            "/mcp/docs",
            &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list"}),
        )
        .await;

        assert!(init.contains("2025-06-18"), "{init}");
        assert!(notified.starts_with("HTTP/1.1 202"), "{notified}");
        assert!(list.contains("whoami"), "{list}");
        assert!(
            call.contains("docs reached; credential line: Authorization: Bearer secret"),
            "{call}"
        );
        assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
        server.abort();
    }
}
