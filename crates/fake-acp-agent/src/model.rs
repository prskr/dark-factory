//! The `model` step: one streamed Anthropic Messages request through the gateway's inference
//! endpoint (iter3-inference-gateway). Uses `curl` like the `fetch` step.

use serde::{Deserialize, Serialize};

/// Scripted model call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelStep {
    pub prompt: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_model() -> String {
    "fake-model".to_string()
}

fn default_timeout() -> u64 {
    60
}

/// What the call needs from the harness environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelEnv {
    /// `ANTHROPIC_BASE_URL`: the gateway's inference endpoint.
    pub base_url: String,
    /// `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN`: the placeholder.
    pub key: String,
    /// `NODE_EXTRA_CA_CERTS`: the instance CA.
    pub ca_file: Option<String>,
}

impl ModelEnv {
    /// Reads the environment the control plane gave the harness.
    pub fn from_env() -> Result<Self, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Ok(Self {
            base_url: var("ANTHROPIC_BASE_URL").ok_or("ANTHROPIC_BASE_URL is not set")?,
            key: var("ANTHROPIC_API_KEY")
                .or_else(|| var("CLAUDE_CODE_OAUTH_TOKEN"))
                .ok_or("neither ANTHROPIC_API_KEY nor CLAUDE_CODE_OAUTH_TOKEN is set")?,
            ca_file: var("NODE_EXTRA_CA_CERTS"),
        })
    }
}

/// The text the model answered, or the failure: `Model call failed: <status or error>`.
pub async fn call(step: &ModelStep, env: &ModelEnv) -> Result<String, String> {
    request(step, env)
        .await
        .map_err(|error| format!("Model call failed: {error}"))
}

async fn request(step: &ModelStep, env: &ModelEnv) -> Result<String, String> {
    let body = serde_json::json!({
        "model": step.model,
        "max_tokens": 64,
        "stream": true,
        "messages": [{"role": "user", "content": step.prompt}],
    });
    let url = format!("{}/v1/messages", env.base_url.trim_end_matches('/'));
    let mut command = tokio::process::Command::new("curl");
    command
        .args(["--silent", "--show-error", "--no-buffer"])
        .args(["--max-time", &step.timeout_seconds.to_string()])
        .args(["--write-out", "\n%{http_code}"])
        .args(["--header", &format!("x-api-key: {}", env.key)])
        .args(["--header", "anthropic-version: 2023-06-01"])
        .args(["--header", "content-type: application/json"])
        .args(["--data", &body.to_string()]);
    if let Some(ca) = &env.ca_file {
        command.args(["--cacert", ca]);
    }
    let output = command
        .arg("--url")
        .arg(url)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (answer, status) = stdout.rsplit_once('\n').unwrap_or(("", &stdout));
    if status.trim() != "200" {
        return Err(status.trim().to_owned());
    }
    Ok(text_of(answer))
}

/// The text of the `content_block_delta` events of a server-sent-events body.
pub(crate) fn text_of(events: &str) -> String {
    events
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data.trim()).ok())
        .filter(|event| {
            event.get("type").and_then(|kind| kind.as_str()) == Some("content_block_delta")
        })
        .filter_map(|event| event.pointer("/delta/text")?.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    const SSE: &str = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hallo \"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}\n\n";

    /// Reads one request: its head and body, as text.
    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut received = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            received.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&received);
            let Some((head, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let length = head
                .to_ascii_lowercase()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")?
                        .trim()
                        .parse::<usize>()
                        .ok()
                })
                .unwrap_or(0);
            if body.len() >= length {
                return text.into_owned();
            }
        }
    }

    /// A one-shot HTTP server; returns its URL and what it received.
    fn serve(
        status: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let received = read_request(&mut stream);
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            received
        });
        (url, handle)
    }

    fn step() -> ModelStep {
        ModelStep {
            prompt: "hi".into(),
            model: "fake-model".into(),
            timeout_seconds: 30,
        }
    }

    fn env(base_url: String) -> ModelEnv {
        ModelEnv {
            base_url,
            key: "df-placeholder".into(),
            ca_file: None,
        }
    }

    #[tokio::test]
    async fn the_streamed_text_is_returned_and_the_request_carries_the_placeholder_key() {
        let (url, server) = serve("200 OK", SSE);

        let text = call(&step(), &env(format!("{url}/inference")))
            .await
            .unwrap();

        assert_eq!(text, "hallo world");
        let request = server.join().unwrap();
        assert!(
            request.starts_with("POST /inference/v1/messages HTTP/1.1"),
            "{request}"
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-api-key: df-placeholder")
        );
        assert!(request.contains("\"stream\":true") && request.contains("\"content\":\"hi\""));
    }

    #[tokio::test]
    async fn a_failed_request_says_the_status() {
        let (url, server) = serve("429 Too Many Requests", "{}");

        let error = call(&step(), &env(url)).await.unwrap_err();

        assert_eq!(error, "Model call failed: 429");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn an_unreachable_gateway_says_the_error() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);

        let error = call(&step(), &env(url)).await.unwrap_err();

        assert!(error.starts_with("Model call failed: "), "{error}");
    }
}
