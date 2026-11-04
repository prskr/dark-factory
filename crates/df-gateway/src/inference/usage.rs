//! Token usage read from a copy of a model response (iter3-inference-gateway design D3).
//!
//! One parser per wire API, fed with the bytes that are relayed to the harness. Server-sent
//! events are read line by line, JSON bodies as a whole; state stays bounded (a line above
//! [`MAX_LINE_BYTES`] and a body above [`MAX_JSON_BYTES`] are skipped, so their usage is
//! unknown).

use df_core::model_provider::WireApi;
use serde_json::Value;

/// Longest server-sent-events line that is parsed.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Largest JSON body that is parsed.
pub const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;

/// Token counts of one request. Cached input tokens count as `cache_read`, not as `input`
/// (Anthropic's semantics).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// What a response said about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Measured {
    pub model: Option<String>,
    /// `None` if the response carried no usage: unknown, not zero.
    pub usage: Option<Usage>,
}

/// Reads usage from a response of one wire API.
#[derive(Debug)]
pub struct UsageParser {
    state: State,
    mode: Mode,
}

#[derive(Debug)]
enum Mode {
    /// `text/event-stream`: complete lines are parsed as they arrive.
    Events { line: Vec<u8>, skipping: bool },
    /// Any other body: parsed at the end.
    Json { body: Vec<u8>, overflow: bool },
    /// A body that is not readable (compressed): usage stays unknown.
    Off,
}

impl UsageParser {
    /// A parser for a response with `content_type` and `content_encoding`.
    pub fn new(api: WireApi, content_type: Option<&str>, content_encoding: Option<&str>) -> Self {
        let compressed = content_encoding
            .is_some_and(|encoding| !encoding.trim().eq_ignore_ascii_case("identity"));
        let events = content_type.is_some_and(|value| {
            value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("text/event-stream")
        });
        let mode = if compressed {
            Mode::Off
        } else if events {
            Mode::Events {
                line: Vec::new(),
                skipping: false,
            }
        } else {
            Mode::Json {
                body: Vec::new(),
                overflow: false,
            }
        };
        Self {
            state: State::new(api),
            mode,
        }
    }

    /// Takes the next bytes of the body.
    pub fn feed(&mut self, chunk: &[u8]) {
        match &mut self.mode {
            Mode::Events { line, skipping } => {
                for piece in chunk.split_inclusive(|byte| *byte == b'\n') {
                    Self::feed_line(&mut self.state, line, skipping, piece);
                }
            }
            Mode::Json { body, overflow } => {
                if *overflow || body.len() + chunk.len() > MAX_JSON_BYTES {
                    *overflow = true;
                    body.clear();
                } else {
                    body.extend_from_slice(chunk);
                }
            }
            Mode::Off => {}
        }
    }

    /// Adds `piece` (up to and including a line end, or the rest of a chunk) to the current line
    /// and parses the line once it is complete. A line above [`MAX_LINE_BYTES`] is skipped.
    fn feed_line(state: &mut State, line: &mut Vec<u8>, skipping: &mut bool, piece: &[u8]) {
        if !*skipping && line.len() + piece.len() > MAX_LINE_BYTES {
            *skipping = true;
            line.clear();
        }
        if !*skipping {
            line.extend_from_slice(piece);
        }
        if piece.ends_with(b"\n") {
            if !*skipping {
                state.event_line(line);
            }
            line.clear();
            *skipping = false;
        }
    }

    /// The end of the body: what was measured.
    pub fn finish(mut self) -> Measured {
        match std::mem::replace(&mut self.mode, Mode::Off) {
            Mode::Events { line, skipping } => {
                if !skipping {
                    self.state.event_line(&line);
                }
            }
            Mode::Json { body, overflow } => {
                if !overflow && let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    self.state.body(&value);
                }
            }
            Mode::Off => {}
        }
        Measured {
            model: self.state.model,
            usage: self.state.usage,
        }
    }
}

#[derive(Debug)]
struct State {
    api: WireApi,
    model: Option<String>,
    usage: Option<Usage>,
}

impl State {
    const fn new(api: WireApi) -> Self {
        Self {
            api,
            model: None,
            usage: None,
        }
    }

    /// One line of an event stream; only `data:` lines with JSON matter.
    fn event_line(&mut self, line: &[u8]) {
        let Ok(text) = std::str::from_utf8(line) else {
            return;
        };
        let Some(data) = text.trim_end_matches(['\r', '\n']).strip_prefix("data:") else {
            return;
        };
        if let Ok(value) = serde_json::from_str::<Value>(data.trim_start()) {
            self.event(&value);
        }
    }

    fn event(&mut self, event: &Value) {
        match self.api {
            WireApi::AnthropicMessages => match str_at(event, &["type"]) {
                Some("message_start") => {
                    self.model = string_at(event, &["message", "model"]).or(self.model.take());
                    if let Some(usage) = event.pointer("/message/usage") {
                        self.usage = Some(anthropic_usage(usage));
                    }
                }
                Some("message_delta") => self.anthropic_delta(event),
                _ => {}
            },
            WireApi::OpenAiChat => self.chat(event),
            WireApi::OpenAiResponses => {
                if let Some(response) = event.get("response") {
                    self.responses(response);
                }
            }
        }
    }

    /// The cumulative counts of a `message_delta` replace the ones of `message_start`.
    fn anthropic_delta(&mut self, event: &Value) {
        let Some(delta) = event.get("usage") else {
            return;
        };
        let usage = self.usage.get_or_insert_with(Usage::default);
        if let Some(output) = delta.get("output_tokens").and_then(Value::as_u64) {
            usage.output = output;
        }
        if let Some(input) = delta
            .get("input_tokens")
            .and_then(Value::as_u64)
            .filter(|input| *input > 0)
        {
            usage.input = input;
        }
    }

    fn body(&mut self, body: &Value) {
        match self.api {
            WireApi::AnthropicMessages => {
                self.model = string_at(body, &["model"]);
                if let Some(usage) = body.get("usage").filter(|usage| usage.is_object()) {
                    self.usage = Some(anthropic_usage(usage));
                }
            }
            WireApi::OpenAiChat => self.chat(body),
            WireApi::OpenAiResponses => self.responses(body),
        }
    }

    /// A chat completion or chunk: `model`, and `usage` unless it is null (chunks without the
    /// final usage chunk).
    fn chat(&mut self, value: &Value) {
        self.model = string_at(value, &["model"]).or(self.model.take());
        if let Some(usage) = value.get("usage").filter(|usage| usage.is_object()) {
            self.usage = Some(openai_usage(
                usage,
                "prompt_tokens",
                "completion_tokens",
                "prompt_tokens_details",
            ));
        }
    }

    /// A response object (`response.completed.response` or a JSON body).
    fn responses(&mut self, response: &Value) {
        self.model = string_at(response, &["model"]).or(self.model.take());
        if let Some(usage) = response.get("usage").filter(|usage| usage.is_object()) {
            self.usage = Some(openai_usage(
                usage,
                "input_tokens",
                "output_tokens",
                "input_tokens_details",
            ));
        }
    }
}

fn anthropic_usage(usage: &Value) -> Usage {
    let count = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input: count("input_tokens"),
        output: count("output_tokens"),
        cache_read: count("cache_read_input_tokens"),
        cache_write: count("cache_creation_input_tokens"),
    }
}

/// `OpenAI` counts cached tokens inside the input; they are reported as cache read instead.
fn openai_usage(usage: &Value, input: &str, output: &str, details: &str) -> Usage {
    let count = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    let cached = usage
        .get(details)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        input: count(input).saturating_sub(cached),
        output: count(output),
        cache_read: cached,
        cache_write: 0,
    }
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |value, key| value.get(*key))?
        .as_str()
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    str_at(value, path).map(str::to_string)
}

#[cfg(test)]
mod tests;
