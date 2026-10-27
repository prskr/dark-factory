//! ACP smoke tests against a fake OpenAI-compatible SSE endpoint.
//!
//! With `DF_HARNESS_LIVE_BASE_URL` / `DF_HARNESS_LIVE_API_KEY` / `DF_HARNESS_LIVE_MODEL` set,
//! `live_endpoint_streams_thoughts_and_text` also runs against the real spike endpoint.

use std::process::Stdio;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// What the fake endpoint answers to one request.
#[derive(Clone)]
enum Reply {
    /// A complete response.
    Plain(&'static str, &'static str, String),
}

/// Serve `replies` in request order (the last one repeats); returns the base URL.
async fn fake_endpoint(replies: Vec<Reply>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut n = 0;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let reply = replies[n.min(replies.len() - 1)].clone();
            n += 1;
            tokio::spawn(async move {
                let mut buf = vec![0u8; 1 << 20];
                let _ = sock.read(&mut buf).await;
                let resp = match reply {
                    Reply::Plain(status, ct, ref body) => format!(
                        "HTTP/1.1 {status}\r\ncontent-type: {ct}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                };
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/v1")
}

fn sse_body(chunks: &[Value]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

fn sse(chunks: &[Value]) -> Reply {
    Reply::Plain("200 OK", "text/event-stream", sse_body(chunks))
}

fn reasoning(text: &str) -> Value {
    json!({"choices":[{"delta":{"reasoning_content":text},"finish_reason":null}]})
}

fn stop_chunk() -> Value {
    json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}})
}

fn tool_call_chunks(name: &str, args: Value) -> Vec<Value> {
    vec![
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}),
        json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}}),
    ]
}

/// ACP client over the child's stdio.
struct Client {
    _child: Child,
    stdin: ChildStdin,
    out: Lines<BufReader<ChildStdout>>,
    seen: Vec<Value>,
    next_id: u64,
    dir: tempfile::TempDir,
}

impl Client {
    async fn start(base_url: &str, api_key: &str, model: &str) -> anyhow::Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_df-harness"))
            .env("DF_HARNESS_BASE_URL", base_url)
            .env("DF_HARNESS_API_KEY", api_key)
            .env("DF_HARNESS_MODEL", model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap()).lines();
        Ok(Self {
            _child: child,
            stdin,
            out,
            seen: Vec::new(),
            next_id: 0,
            dir: tempfile::tempdir()?,
        })
    }

    async fn send(&mut self, msg: Value) -> anyhow::Result<()> {
        self.stdin.write_all(format!("{msg}\n").as_bytes()).await?;
        Ok(())
    }

    async fn request_id(&mut self, method: &str, params: Value) -> anyhow::Result<u64> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        Ok(id)
    }

    /// Read lines until `pred` matches one; returns it.
    async fn read_until(&mut self, pred: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        while let Some(line) = self.out.next_line().await? {
            let v: Value = serde_json::from_str(&line)?;
            self.seen.push(v.clone());
            if pred(&v) {
                return Ok(v);
            }
        }
        anyhow::bail!("stdout closed; seen: {:?}", self.seen)
    }

    async fn response(&mut self, id: u64) -> anyhow::Result<Value> {
        self.read_until(|v| v.get("id") == Some(&json!(id))).await
    }

    async fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.request_id(method, params).await?;
        self.response(id).await
    }

    async fn new_session(&mut self) -> anyhow::Result<Value> {
        self.call("initialize", json!({"protocolVersion":1}))
            .await?;
        let cwd = self.dir.path().to_owned();
        let new = self
            .call("session/new", json!({"cwd":cwd,"mcpServers":[]}))
            .await?;
        Ok(new["result"]["sessionId"].clone())
    }

    async fn prompt_request(&mut self, sid: &Value, text: &str) -> anyhow::Result<u64> {
        self.request_id(
            "session/prompt",
            json!({"sessionId":sid,"prompt":[{"type":"text","text":text}]}),
        )
        .await
    }

    fn updates(&self) -> Vec<&str> {
        self.seen
            .iter()
            .filter_map(|v| v["params"]["update"]["sessionUpdate"].as_str())
            .collect()
    }
}

async fn one_prompt(base_url: &str) -> anyhow::Result<Client> {
    let mut c = Client::start(base_url, "k", "m").await?;
    let sid = c.new_session().await?;
    let id = c.prompt_request(&sid, "hi").await?;
    c.response(id).await?;
    Ok(c)
}

#[tokio::test]
async fn streams_thoughts_and_text_then_ends_turn() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![sse(&[
        reasoning("thinking"),
        json!({"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}),
        stop_chunk(),
    ])])
    .await;
    let c = one_prompt(&url).await?;
    let kinds = c.updates();
    assert!(kinds.contains(&"agent_thought_chunk"), "{:?}", c.seen);
    assert!(kinds.contains(&"agent_message_chunk"), "{:?}", c.seen);
    assert_eq!(c.seen.last().unwrap()["result"]["stopReason"], "end_turn");
    Ok(())
}

#[tokio::test]
async fn reports_usage_after_a_turn() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![sse(&[
        json!({"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}),
        stop_chunk(),
    ])])
    .await;
    let c = one_prompt(&url).await?;
    let usage = c
        .seen
        .iter()
        .map(|v| &v["params"]["update"])
        .find(|u| u["sessionUpdate"] == "usage_update")
        .unwrap_or_else(|| panic!("no usage_update: {:?}", c.seen));
    assert_eq!(usage["used"], 5);
    assert_eq!(usage["size"], 128_000);
    Ok(())
}

#[tokio::test]
async fn runs_a_tool_call_and_continues() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![
        sse(&tool_call_chunks(
            "bash",
            json!({"command":"echo hi > out.txt"}),
        )),
        sse(&[
            json!({"choices":[{"delta":{"content":"done"},"finish_reason":null}]}),
            stop_chunk(),
        ]),
    ])
    .await;
    let c = one_prompt(&url).await?;
    let kinds = c.updates();
    assert!(kinds.contains(&"tool_call"), "{:?}", c.seen);
    let finished = c
        .seen
        .iter()
        .map(|v| &v["params"]["update"])
        .any(|u| u["sessionUpdate"] == "tool_call_update" && u["status"] == "completed");
    assert!(finished, "{:?}", c.seen);
    assert_eq!(
        std::fs::read_to_string(c.dir.path().join("out.txt"))?.trim(),
        "hi"
    );
    assert_eq!(c.seen.last().unwrap()["result"]["stopReason"], "end_turn");
    Ok(())
}

#[tokio::test]
async fn cancel_during_a_tool_call_ends_the_turn_as_cancelled() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![sse(&tool_call_chunks(
        "bash",
        json!({"command":"sleep 60"}),
    ))])
    .await;
    let mut c = Client::start(&url, "k", "m").await?;
    let sid = c.new_session().await?;
    let id = c.prompt_request(&sid, "hi").await?;
    c.read_until(|v| v["params"]["update"]["sessionUpdate"] == "tool_call")
        .await?;
    c.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":sid}}))
        .await?;
    let res = tokio::time::timeout(std::time::Duration::from_secs(20), c.response(id)).await??;
    assert_eq!(res["result"]["stopReason"], "cancelled", "{res}");
    Ok(())
}

#[tokio::test]
async fn failing_prompt_keeps_its_error_message() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![sse(&tool_call_chunks(
        "bash",
        json!({"command":"sleep 60"}),
    ))])
    .await;
    let mut c = Client::start(&url, "k", "m").await?;
    let sid = c.new_session().await?;
    let first = c.prompt_request(&sid, "hi").await?;
    c.read_until(|v| v["params"]["update"]["sessionUpdate"] == "tool_call")
        .await?;
    // The session is busy, so the second prompt fails inside rpi-agent.
    let second = c.prompt_request(&sid, "again").await?;
    let res =
        tokio::time::timeout(std::time::Duration::from_secs(10), c.response(second)).await??;
    assert!(
        res["error"]["data"]
            .to_string()
            .contains("already processing"),
        "{res}"
    );
    c.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":sid}}))
        .await?;
    let res = tokio::time::timeout(std::time::Duration::from_secs(20), c.response(first)).await??;
    assert_eq!(res["result"]["stopReason"], "cancelled", "{res}");
    Ok(())
}

#[tokio::test]
async fn rate_limit_surfaces_as_rate_limited_error() -> anyhow::Result<()> {
    let url = fake_endpoint(vec![Reply::Plain(
        "429 Too Many Requests",
        "application/json",
        r#"{"error":{"message":"slow down"}}"#.into(),
    )])
    .await;
    let c = one_prompt(&url).await?;
    assert_eq!(
        c.seen.last().unwrap()["error"]["data"]["error"],
        "rate_limited",
        "{:?}",
        c.seen
    );
    Ok(())
}

/// Against the real spike endpoint; skipped unless `DF_HARNESS_LIVE_*` is set.
#[tokio::test]
async fn live_endpoint_streams_thoughts_and_text() -> anyhow::Result<()> {
    let (Ok(url), Ok(key), Ok(model)) = (
        std::env::var("DF_HARNESS_LIVE_BASE_URL"),
        std::env::var("DF_HARNESS_LIVE_API_KEY"),
        std::env::var("DF_HARNESS_LIVE_MODEL"),
    ) else {
        return Ok(());
    };
    let mut c = Client::start(&url, &key, &model).await?;
    let sid = c.new_session().await?;
    let id = c.prompt_request(&sid, "Say hello in one word.").await?;
    c.response(id).await?;
    let kinds = c.updates();
    assert!(kinds.contains(&"agent_thought_chunk"), "{:?}", c.seen);
    assert!(kinds.contains(&"agent_message_chunk"), "{:?}", c.seen);
    assert_eq!(c.seen.last().unwrap()["result"]["stopReason"], "end_turn");
    Ok(())
}
