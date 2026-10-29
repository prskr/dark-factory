//! ACP → Dark Factory transcript translation.
//!
//! Follows `specs/acp-harness/spec.md`, `design/agent-run.md` §5.2, and ADR-0017.

use agent_client_protocol::schema::v1::{
    ContentBlock as AcpContentBlock, ContentChunk, Plan as AcpPlan,
    PlanEntryPriority as AcpPlanPriority, PlanEntryStatus as AcpPlanStatus,
    StopReason as AcpStopReason, ToolCall as AcpToolCall, ToolCallContent as AcpToolCallContent,
    ToolCallLocation as AcpToolCallLocation, ToolCallStatus as AcpToolCallStatus,
    ToolCallUpdate as AcpToolCallUpdate, ToolKind as AcpToolKind,
};
use df_proto::transcript::v1::{
    AgentMessageChunk, AgentThoughtChunk, ContentBlock, Event, Location, LocationList, Plan,
    PlanEntry, PlanEntryPriority, PlanEntryStatus, StopReason, TextContent, ToolCall,
    ToolCallContent, ToolCallContentList, ToolCallStatus, ToolCallUpdate, ToolKind, ToolOrigin,
    TurnEnded, Usage, UsageSource, content_block::Kind as ContentBlockKind,
    event::Kind as EventKind,
};

/// Builds a transcript event carrying `notice`.
pub fn notice_event(notice: df_proto::transcript::v1::Notice) -> Event {
    Event {
        schema_version: 1,
        occurred_at: Some(chrono::Utc::now().into()),
        turn_id: String::new(),
        kind: Some(EventKind::Notice(notice)),
    }
}

/// Maximum raw input/output length in bytes before truncation (64 KiB).
pub const MAX_RAW_IO_BYTES: usize = 64 * 1024;

/// Truncates string to `MAX_RAW_IO_BYTES` on a valid UTF-8 boundary.
///
/// Returns `(truncated_str, was_truncated)`.
pub fn truncate_raw_io(s: &str) -> (String, bool) {
    let (kept, truncated) = truncate_utf8(s, MAX_RAW_IO_BYTES);
    (kept.to_string(), truncated)
}

/// Cuts `s` to at most `max` bytes on a UTF-8 character boundary.
pub fn truncate_utf8(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

fn map_status(status: AcpToolCallStatus) -> ToolCallStatus {
    match status {
        AcpToolCallStatus::Pending => ToolCallStatus::Pending,
        AcpToolCallStatus::InProgress => ToolCallStatus::InProgress,
        AcpToolCallStatus::Completed => ToolCallStatus::Completed,
        AcpToolCallStatus::Failed => ToolCallStatus::Failed,
        _ => ToolCallStatus::Unspecified,
    }
}

fn map_kind(kind: AcpToolKind) -> ToolKind {
    match kind {
        AcpToolKind::Read => ToolKind::Read,
        AcpToolKind::Edit => ToolKind::Edit,
        AcpToolKind::Execute => ToolKind::Execute,
        AcpToolKind::Search => ToolKind::Search,
        AcpToolKind::Think => ToolKind::Think,
        AcpToolKind::Other => ToolKind::Other,
        _ => ToolKind::Unspecified,
    }
}

/// Tracks event kind to detect resumption of text or thought chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LastEventKind {
    AgentMessage,
    AgentThought,
    Other,
}

/// State machine translating streaming ACP updates into Dark Factory transcript events.
#[derive(Debug)]
pub struct TranscriptTranslator {
    turn_id: String,
    current_message_id: Option<String>,
    current_thought_id: Option<String>,
    last_event_kind: Option<LastEventKind>,
    message_counter: usize,
    thought_counter: usize,
}

impl TranscriptTranslator {
    /// Create a translator for the given `turn_id`.
    pub fn new(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
            current_message_id: None,
            current_thought_id: None,
            last_event_kind: None,
            message_counter: 0,
            thought_counter: 0,
        }
    }

    /// Update the current turn ID when starting a new turn.
    pub fn set_turn_id(&mut self, turn_id: impl Into<String>) {
        self.turn_id = turn_id.into();
        self.current_message_id = None;
        self.current_thought_id = None;
        self.last_event_kind = None;
    }

    /// Translates an incoming ACP agent message chunk.
    ///
    /// The `message_id` changes whenever agent text resumes after another event (e.g. tool call or thought).
    pub fn translate_agent_message_chunk(&mut self, chunk: &ContentChunk) -> Event {
        if self.last_event_kind != Some(LastEventKind::AgentMessage) {
            self.message_counter += 1;
            self.current_message_id = Some(format!("msg-{}", self.message_counter));
            self.last_event_kind = Some(LastEventKind::AgentMessage);
        }

        let content = convert_acp_content_block_to_proto(&chunk.content);
        let message_id = self.current_message_id.clone().unwrap_or_default();

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::AgentMessageChunk(AgentMessageChunk {
                message_id,
                content: Some(content),
            })),
        }
    }

    /// Translates an incoming ACP agent thought chunk.
    ///
    /// The `message_id` changes whenever thought text resumes after another event.
    pub fn translate_agent_thought_chunk(&mut self, chunk: &ContentChunk) -> Event {
        if self.last_event_kind != Some(LastEventKind::AgentThought) {
            self.thought_counter += 1;
            self.current_thought_id = Some(format!("thought-{}", self.thought_counter));
            self.last_event_kind = Some(LastEventKind::AgentThought);
        }

        let content = convert_acp_content_block_to_proto(&chunk.content);
        let message_id = self.current_thought_id.clone().unwrap_or_default();

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::AgentThoughtChunk(AgentThoughtChunk {
                message_id,
                content: Some(content),
            })),
        }
    }

    /// Translates an incoming ACP tool call initiation.
    pub fn translate_tool_call(&mut self, call: &AcpToolCall) -> Event {
        self.last_event_kind = Some(LastEventKind::Other);

        let title = call.title.clone();
        let name_str = call.name.as_deref().unwrap_or("");
        let is_factory =
            title.starts_with("mcp__factory__") || name_str.starts_with("mcp__factory__");

        let origin = if is_factory {
            ToolOrigin::Factory
        } else {
            ToolOrigin::Harness
        };

        let raw_input_str = call
            .raw_input
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let (raw_input_json, input_trunc) = truncate_raw_io(&raw_input_str);

        let raw_output_str = call
            .raw_output
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        let (raw_output_json, output_trunc) = truncate_raw_io(&raw_output_str);

        let status = map_status(call.status);
        let kind = map_kind(call.kind);

        let mut content = Vec::new();
        let mut content_trunc = false;
        for c in &call.content {
            let (proto_c, trunc) = convert_acp_tool_content_to_proto(c);
            content.push(proto_c);
            content_trunc |= trunc;
        }
        let truncated = input_trunc || output_trunc || content_trunc;

        let locations = call
            .locations
            .iter()
            .map(convert_acp_location_to_proto)
            .collect();

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::ToolCall(ToolCall {
                tool_call_id: call.tool_call_id.to_string(),
                title,
                kind: kind as i32,
                status: status as i32,
                content,
                locations,
                raw_input_json,
                raw_output_json,
                origin: origin as i32,
                factory_call_id: String::new(),
                truncated,
            })),
        }
    }

    /// Translates an incoming ACP tool call update.
    pub fn translate_tool_call_update(&mut self, update: &AcpToolCallUpdate) -> Event {
        self.last_event_kind = Some(LastEventKind::Other);

        let f = &update.fields;

        let raw_input_json = f.raw_input.as_ref().map(|v| {
            let s = serde_json::to_string(v).unwrap_or_default();
            truncate_raw_io(&s)
        });

        let raw_output_json = f.raw_output.as_ref().map(|v| {
            let s = serde_json::to_string(v).unwrap_or_default();
            truncate_raw_io(&s)
        });

        let mut truncated = false;
        let input_str = raw_input_json.map(|(s, t)| {
            truncated |= t;
            s
        });
        let output_str = raw_output_json.map(|(s, t)| {
            truncated |= t;
            s
        });

        let status = f.status.map(|s| map_status(s) as i32);
        let kind = f.kind.map(|k| map_kind(k) as i32);

        let mut content_trunc = false;
        let content_list = f.content.as_ref().map(|items| {
            let mut list = Vec::new();
            for item in items {
                let (proto_c, trunc) = convert_acp_tool_content_to_proto(item);
                list.push(proto_c);
                content_trunc |= trunc;
            }
            ToolCallContentList { items: list }
        });
        truncated |= content_trunc;

        let location_list = f.locations.as_ref().map(|items| LocationList {
            items: items.iter().map(convert_acp_location_to_proto).collect(),
        });

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::ToolCallUpdate(ToolCallUpdate {
                tool_call_id: update.tool_call_id.to_string(),
                title: f.title.clone(),
                kind,
                status,
                content: content_list,
                locations: location_list,
                raw_input_json: input_str,
                raw_output_json: output_str,
                truncated: if truncated { Some(true) } else { None },
            })),
        }
    }

    /// Translates an incoming ACP plan.
    pub fn translate_plan(&mut self, plan: &AcpPlan) -> Event {
        self.last_event_kind = Some(LastEventKind::Other);

        let entries = plan
            .entries
            .iter()
            .map(|e| {
                let priority = match e.priority {
                    AcpPlanPriority::High => PlanEntryPriority::High,
                    AcpPlanPriority::Medium => PlanEntryPriority::Medium,
                    AcpPlanPriority::Low => PlanEntryPriority::Low,
                    _ => PlanEntryPriority::Unspecified,
                };
                let status = match e.status {
                    AcpPlanStatus::Pending => PlanEntryStatus::Pending,
                    AcpPlanStatus::InProgress => PlanEntryStatus::InProgress,
                    AcpPlanStatus::Completed => PlanEntryStatus::Completed,
                    _ => PlanEntryStatus::Unspecified,
                };
                PlanEntry {
                    content: e.content.clone(),
                    priority: priority as i32,
                    status: status as i32,
                }
            })
            .collect();

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::Plan(Plan { entries })),
        }
    }

    /// Translates turn completion with explicit Dark Factory `StopReason` into a `TurnEnded` event.
    pub fn translate_turn_ended_with_stop_reason(
        &mut self,
        stop_reason: StopReason,
        usage: Vec<Usage>,
        usage_source: UsageSource,
        detail: &str,
    ) -> Event {
        self.last_event_kind = Some(LastEventKind::Other);

        Event {
            schema_version: 1,
            occurred_at: Some(chrono::Utc::now().into()),
            turn_id: self.turn_id.clone(),
            kind: Some(EventKind::TurnEnded(TurnEnded {
                stop_reason: stop_reason as i32,
                usage,
                usage_source: usage_source as i32,
                detail: detail.to_string(),
            })),
        }
    }

    /// Translates turn completion into a `TurnEnded` event.
    #[cfg(test)]
    pub fn translate_turn_ended(
        &mut self,
        stop_reason: AcpStopReason,
        usage: Vec<Usage>,
        usage_source: UsageSource,
        detail: &str,
    ) -> Event {
        let reason = acp_to_proto_stop_reason(stop_reason);
        self.translate_turn_ended_with_stop_reason(reason, usage, usage_source, detail)
    }
}

/// Converts an ACP stop reason to a Dark Factory proto `StopReason`.
pub fn acp_to_proto_stop_reason(stop_reason: AcpStopReason) -> StopReason {
    match stop_reason {
        AcpStopReason::EndTurn => StopReason::EndTurn,
        AcpStopReason::MaxTokens => StopReason::MaxTokens,
        AcpStopReason::Cancelled => StopReason::Cancelled,
        AcpStopReason::Refusal => StopReason::Refusal,
        AcpStopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
        _ => StopReason::Unspecified,
    }
}

fn convert_acp_content_block_to_proto(block: &AcpContentBlock) -> ContentBlock {
    match block {
        AcpContentBlock::Text(t) => ContentBlock {
            kind: Some(ContentBlockKind::Text(TextContent {
                text: t.text.clone(),
            })),
            hidden: false,
        },
        _ => ContentBlock {
            kind: Some(ContentBlockKind::Text(TextContent {
                text: String::new(),
            })),
            hidden: false,
        },
    }
}

fn convert_acp_tool_content_to_proto(tc: &AcpToolCallContent) -> (ToolCallContent, bool) {
    match tc {
        AcpToolCallContent::Content(c) => match &c.content {
            AcpContentBlock::Text(t) => {
                let (text, trunc) = truncate_raw_io(&t.text);
                (
                    ToolCallContent {
                        kind: Some(df_proto::transcript::v1::tool_call_content::Kind::Content(
                            ContentBlock {
                                kind: Some(ContentBlockKind::Text(TextContent { text })),
                                hidden: false,
                            },
                        )),
                    },
                    trunc,
                )
            }
            other => (
                ToolCallContent {
                    kind: Some(df_proto::transcript::v1::tool_call_content::Kind::Content(
                        convert_acp_content_block_to_proto(other),
                    )),
                },
                false,
            ),
        },
        _ => (
            ToolCallContent {
                kind: Some(df_proto::transcript::v1::tool_call_content::Kind::Content(
                    ContentBlock {
                        kind: Some(ContentBlockKind::Text(TextContent {
                            text: String::new(),
                        })),
                        hidden: false,
                    },
                )),
            },
            false,
        ),
    }
}

fn convert_acp_location_to_proto(loc: &AcpToolCallLocation) -> Location {
    Location {
        path: loc.path.to_string_lossy().to_string(),
        line: loc.line,
    }
}
