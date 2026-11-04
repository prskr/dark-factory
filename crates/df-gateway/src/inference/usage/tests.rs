use super::*;

const EVENTS: &str = "text/event-stream";
const JSON: &str = "application/json";

/// Feeds `body` in chunks of `size` bytes.
fn measure(api: WireApi, content_type: &str, body: &str, size: usize) -> Measured {
    let mut parser = UsageParser::new(api, Some(content_type), None);
    for chunk in body.as_bytes().chunks(size) {
        parser.feed(chunk);
    }
    parser.finish()
}

fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Usage {
    Usage {
        input,
        output,
        cache_read,
        cache_write,
    }
}

const ANTHROPIC_STREAM: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-sonnet-5-5\",",
    "\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":15,\"cache_creation_input_tokens\":7,\"output_tokens\":1}}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":40}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

#[test]
fn streamed_anthropic_message_reports_start_counts_and_the_final_output() {
    for size in [1, 7, 64, usize::MAX >> 1] {
        let measured = measure(WireApi::AnthropicMessages, EVENTS, ANTHROPIC_STREAM, size);
        assert_eq!(
            measured.model.as_deref(),
            Some("claude-sonnet-5-5"),
            "{size}"
        );
        assert_eq!(measured.usage, Some(usage(120, 40, 15, 7)), "{size}");
    }
}

#[test]
fn anthropic_json_body_reports_model_and_usage() {
    let body = r#"{"id":"msg_1","model":"claude-haiku-5-5","content":[],"usage":{"input_tokens":10,"output_tokens":3}}"#;
    let measured = measure(WireApi::AnthropicMessages, JSON, body, 16);
    assert_eq!(measured.model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(measured.usage, Some(usage(10, 3, 0, 0)));
}

#[test]
fn anthropic_error_body_has_unknown_usage() {
    let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
    let measured = measure(WireApi::AnthropicMessages, JSON, body, 1024);
    assert_eq!(measured, Measured::default());
}

#[test]
fn chat_completion_stream_without_a_usage_chunk_is_unknown() {
    let body = concat!(
        "data: {\"model\":\"gpt-x\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":null}\n\n",
        "data: [DONE]\n\n",
    );
    let measured = measure(WireApi::OpenAiChat, EVENTS, body, 5);
    assert_eq!(measured.model.as_deref(), Some("gpt-x"));
    assert_eq!(measured.usage, None);
}

#[test]
fn chat_completion_stream_with_a_usage_chunk_reports_it_without_cached_tokens_in_the_input() {
    let body = concat!(
        "data: {\"model\":\"gpt-x\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
        "data: {\"model\":\"gpt-x\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":9,",
        "\"prompt_tokens_details\":{\"cached_tokens\":60}}}\n\n",
        "data: [DONE]\n\n",
    );
    let measured = measure(WireApi::OpenAiChat, EVENTS, body, 11);
    assert_eq!(measured.usage, Some(usage(40, 9, 60, 0)));
}

#[test]
fn chat_completion_json_body_reports_usage() {
    let body =
        r#"{"model":"gpt-x","choices":[],"usage":{"prompt_tokens":8,"completion_tokens":2}}"#;
    let measured = measure(WireApi::OpenAiChat, JSON, body, 4);
    assert_eq!(measured.usage, Some(usage(8, 2, 0, 0)));
}

#[test]
fn responses_stream_reports_the_completed_event() {
    let body = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-r\",",
        "\"usage\":{\"input_tokens\":50,\"output_tokens\":6,\"input_tokens_details\":{\"cached_tokens\":10}}}}\n\n",
    );
    let measured = measure(WireApi::OpenAiResponses, EVENTS, body, 13);
    assert_eq!(measured.model.as_deref(), Some("gpt-r"));
    assert_eq!(measured.usage, Some(usage(40, 6, 10, 0)));
}

#[test]
fn responses_json_body_reports_usage() {
    let body = r#"{"model":"gpt-r","usage":{"input_tokens":5,"output_tokens":1}}"#;
    let measured = measure(WireApi::OpenAiResponses, JSON, body, 1024);
    assert_eq!(measured.usage, Some(usage(5, 1, 0, 0)));
}

#[test]
fn an_oversized_line_is_skipped_and_the_stream_goes_on() {
    let huge = "x".repeat(MAX_LINE_BYTES + 10);
    let body = format!("data: {{\"type\":\"ping\",\"pad\":\"{huge}\"}}\n\n{ANTHROPIC_STREAM}");
    let measured = measure(WireApi::AnthropicMessages, EVENTS, &body, 4096);
    assert_eq!(measured.usage, Some(usage(120, 40, 15, 7)));
}

#[test]
fn an_oversized_json_body_is_not_parsed() {
    let body = format!(
        "{{\"model\":\"m\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}},\"pad\":\"{}\"}}",
        "x".repeat(MAX_JSON_BYTES)
    );
    let measured = measure(WireApi::AnthropicMessages, JSON, &body, 1 << 20);
    assert_eq!(measured, Measured::default());
}

#[test]
fn a_compressed_body_is_not_readable() {
    let mut parser = UsageParser::new(WireApi::AnthropicMessages, Some(JSON), Some("gzip"));
    parser.feed(b"\x1f\x8b not json");
    assert_eq!(parser.finish(), Measured::default());
}

#[test]
fn a_stream_cut_short_keeps_what_was_seen() {
    let cut = &ANTHROPIC_STREAM[..ANTHROPIC_STREAM.find("event: message_delta").unwrap()];
    let measured = measure(WireApi::AnthropicMessages, EVENTS, cut, 32);
    assert_eq!(measured.usage, Some(usage(120, 1, 15, 7)));
}

#[test]
fn lines_with_crlf_and_a_missing_final_newline_are_read() {
    let body = "data: {\"type\":\"message_start\",\"message\":{\"model\":\"m\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\r\n\r\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":9}}";
    let measured = measure(WireApi::AnthropicMessages, EVENTS, body, 9);
    assert_eq!(measured.usage, Some(usage(2, 9, 0, 0)));
}
