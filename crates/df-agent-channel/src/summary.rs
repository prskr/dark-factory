//! The continuation summary: user and agent text and one line per tool call of the earlier runs
//! of a task, rebuilt on every `Assign` (design D1-D5). Seeds a session that cannot be resumed.
//! *ponytail:* built-in strategy, no model-generated summary.

use std::collections::HashMap;

use df_proto::transcript::v1::ToolCallStatus;
use sqlx::PgPool;
use uuid::Uuid;

use crate::assign::RunContext;
use crate::error::ChannelError;

/// Upper bound of `Continuation.summary`, header included.
pub(crate) const SUMMARY_MAX_BYTES: usize = 64 * 1024;
/// Share of the summary reserved for task title and description.
const SUMMARY_HEADER_MAX_BYTES: usize = 4 * 1024;
const SEPARATOR: &str = "\n\n";

/// Selects the four kinds of other runs as flat columns (no raw tool input or output). The text
/// of a user message holds its visible blocks only (`hidden` blocks are left out).
const EVENTS_QUERY: &str = "
SELECT run_id, kind,
  COALESCE(payload ->> 'turnId', '') AS turn_id,
  COALESCE(payload #>> '{agentMessageChunk,messageId}', '') AS message_id,
  COALESCE(CASE kind
    WHEN 'user_message' THEN
      (SELECT string_agg(b.block #>> '{text,text}', E'\\n' ORDER BY b.ord)
         FROM jsonb_array_elements(payload #> '{userMessage,content}')
              WITH ORDINALITY AS b(block, ord)
        WHERE COALESCE((b.block ->> 'hidden')::boolean, false) = false)
    ELSE payload #>> '{agentMessageChunk,content,text,text}'
  END, '') AS text,
  COALESCE(payload #>> '{toolCall,toolCallId}', payload #>> '{toolCallUpdate,toolCallId}', '')
    AS tool_call_id,
  COALESCE(payload #>> '{toolCall,title}', payload #>> '{toolCallUpdate,title}') AS title,
  COALESCE(payload #>> '{toolCall,status}', payload #>> '{toolCallUpdate,status}') AS status
FROM transcript_events
WHERE task_id = $1 AND run_id <> $2
  AND kind IN ('user_message', 'agent_message_chunk', 'tool_call', 'tool_call_update')
ORDER BY id";

/// A transcript event the fold reads. Which fields are set depends on `kind`: `text` for
/// messages (visible text of a user message), `message_id` for agent chunks, `turn_id`,
/// `tool_call_id`, `title` and `status` for tool events.
#[derive(Debug, Clone)]
pub struct SummaryEvent {
    pub run_id: Uuid,
    pub kind: EventKind,
    pub turn_id: String,
    pub message_id: String,
    pub text: String,
    pub tool_call_id: String,
    pub title: Option<String>,
    pub status: Option<ToolCallStatus>,
}

/// The transcript event kinds the summary reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    UserMessage,
    AgentMessageChunk,
    ToolCall,
    ToolCallUpdate,
}

impl EventKind {
    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "user_message" => Some(Self::UserMessage),
            "agent_message_chunk" => Some(Self::AgentMessageChunk),
            "tool_call" => Some(Self::ToolCall),
            "tool_call_update" => Some(Self::ToolCallUpdate),
            _ => None,
        }
    }
}

/// Folds `events` (oldest first) into entries that fit `budget` bytes (separators included).
/// Consecutive chunks of one agent message are merged, a tool call becomes one line with its last
/// title and status, two or more consecutive completed calls collapse into a counted line, hidden
/// blocks of user messages are left out. At the budget, tool lines and agent messages are dropped
/// oldest first, user messages only when nothing else is left; the newest entry stays.
pub fn fold_summary(events: &[SummaryEvent], budget: usize) -> Vec<String> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut tools: HashMap<ToolKey, usize> = HashMap::new();
    for event in events {
        match event.kind {
            EventKind::UserMessage => {
                if !event.text.is_empty() {
                    entries.push(Entry::User(event.text.clone()));
                }
            }
            EventKind::AgentMessageChunk => fold_chunk(&mut entries, event),
            EventKind::ToolCall | EventKind::ToolCallUpdate => {
                fold_tool(&mut entries, &mut tools, event);
            }
        }
    }
    fit_entries(collapse_completed(entries), budget)
}

/// Task header (cut to [`SUMMARY_HEADER_MAX_BYTES`]) followed by the folded entries, within
/// [`SUMMARY_MAX_BYTES`].
pub(crate) fn render_summary(header: String, events: &[SummaryEvent]) -> String {
    let header = truncate_utf8(header, SUMMARY_HEADER_MAX_BYTES);
    let entries = fold_summary(events, SUMMARY_MAX_BYTES - header.len());
    std::iter::once(header)
        .chain(entries)
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

/// Builds the summary of a task for the run being assigned from all transcript events of the
/// other runs (design D1). Only the payload fields the fold reads are selected.
pub(crate) async fn continuation_summary(
    pool: &PgPool,
    rc: &RunContext,
) -> Result<String, ChannelError> {
    let rows: Vec<EventRow> = sqlx::query_as(EVENTS_QUERY)
        .bind(rc.task.id.as_uuid())
        .bind(rc.run.id.as_uuid())
        .fetch_all(pool)
        .await?;
    let events: Vec<SummaryEvent> = rows
        .into_iter()
        .filter_map(|row| {
            Some(SummaryEvent {
                run_id: row.run_id,
                kind: EventKind::parse(&row.kind)?,
                turn_id: row.turn_id,
                message_id: row.message_id,
                text: row.text,
                tool_call_id: row.tool_call_id,
                title: row.title,
                status: row
                    .status
                    .as_deref()
                    .and_then(ToolCallStatus::from_str_name),
            })
        })
        .collect();
    let header = format!("# {}\n\n{}", rc.task.title, rc.task.description);
    Ok(render_summary(header, &events))
}

/// A row of [`EVENTS_QUERY`], bound by column name.
#[derive(sqlx::FromRow)]
struct EventRow {
    run_id: Uuid,
    kind: String,
    turn_id: String,
    message_id: String,
    text: String,
    tool_call_id: String,
    title: Option<String>,
    status: Option<String>,
}

/// `(run_id, turn_id, tool_call_id)`: the harness id is unique within a turn only.
type ToolKey = (Uuid, String, String);

enum Entry {
    User(String),
    Agent {
        message_id: String,
        text: String,
    },
    Tool {
        title: String,
        status: Option<ToolCallStatus>,
    },
    CompletedTools(usize),
}

impl Entry {
    fn render(&self) -> String {
        match self {
            Self::User(text) => format!("User: {text}"),
            Self::Agent { text, .. } => format!("Agent: {text}"),
            Self::Tool {
                title,
                status: Some(status),
            } => format!("Tool: {title} ({})", status_label(*status)),
            Self::Tool {
                title,
                status: None,
            } => format!("Tool: {title}"),
            Self::CompletedTools(n) => format!("Tool: {n} calls completed"),
        }
    }

    fn is_user(&self) -> bool {
        matches!(self, Self::User(_))
    }

    fn is_completed_tool(&self) -> bool {
        matches!(
            self,
            Self::Tool {
                status: Some(ToolCallStatus::Completed),
                ..
            }
        )
    }
}

/// Replaces each run of two or more adjacent completed tool calls by one counted entry.
fn collapse_completed(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut streak: Vec<Entry> = Vec::new();
    for entry in entries {
        if entry.is_completed_tool() {
            streak.push(entry);
            continue;
        }
        flush_streak(&mut out, &mut streak);
        out.push(entry);
    }
    flush_streak(&mut out, &mut streak);
    out
}

fn flush_streak(out: &mut Vec<Entry>, streak: &mut Vec<Entry>) {
    if streak.len() >= 2 {
        out.push(Entry::CompletedTools(streak.len()));
        streak.clear();
    } else {
        out.append(streak);
    }
}

/// `in progress`, `failed`, ...
fn status_label(status: ToolCallStatus) -> String {
    crate::metrics::label(status.as_str_name(), "TOOL_CALL_STATUS_").replace('_', " ")
}

fn fold_chunk(entries: &mut Vec<Entry>, event: &SummaryEvent) {
    if event.text.is_empty() {
        return;
    }
    match entries.last_mut() {
        Some(Entry::Agent { message_id, text }) if *message_id == event.message_id => {
            text.push_str(&event.text);
        }
        _ => entries.push(Entry::Agent {
            message_id: event.message_id.clone(),
            text: event.text.clone(),
        }),
    }
}

fn fold_tool(entries: &mut Vec<Entry>, tools: &mut HashMap<ToolKey, usize>, event: &SummaryEvent) {
    let key = (
        event.run_id,
        event.turn_id.clone(),
        event.tool_call_id.clone(),
    );
    let title = event.title.as_deref().filter(|t| !t.is_empty());
    let status = event.status.filter(|s| *s != ToolCallStatus::Unspecified);
    if let Some(&at) = tools.get(&key) {
        if let Some(Entry::Tool {
            title: t,
            status: s,
        }) = entries.get_mut(at)
        {
            if let Some(title) = title {
                *t = title.to_string();
            }
            if status.is_some() {
                *s = status;
            }
        }
    } else if event.kind == EventKind::ToolCall {
        tools.insert(key, entries.len());
        entries.push(Entry::Tool {
            title: title.unwrap_or("tool call").to_string(),
            status,
        });
    }
    // An update without its `tool_call` is dropped.
}

/// Drops entries until the rest fits `budget` bytes (separators included): tool lines and agent
/// messages oldest first, then user messages oldest first. The newest entry always stays,
/// shortened if it alone exceeds the budget.
fn fit_entries(entries: Vec<Entry>, budget: usize) -> Vec<String> {
    let mut rendered: Vec<Option<String>> = entries.iter().map(|e| Some(e.render())).collect();
    let mut total: usize = rendered
        .iter()
        .flatten()
        .map(|e| e.len() + SEPARATOR.len())
        .sum();
    let newest = rendered.len().saturating_sub(1);
    for drop_users in [false, true] {
        for (i, entry) in entries.iter().enumerate().take(newest) {
            if total <= budget {
                break;
            }
            if entry.is_user() == drop_users
                && let Some(text) = rendered.get_mut(i).and_then(Option::take)
            {
                total -= text.len() + SEPARATOR.len();
            }
        }
    }
    let mut kept: Vec<String> = rendered.into_iter().flatten().collect();
    if total > budget
        && let Some(last) = kept.pop()
    {
        kept.push(truncate_utf8(last, budget.saturating_sub(SEPARATOR.len())));
    }
    kept
}

fn truncate_utf8(mut s: String, max_bytes: usize) -> String {
    if s.len() > max_bytes {
        let mut end = max_bytes;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

#[cfg(test)]
mod tests;
