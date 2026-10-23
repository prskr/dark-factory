//! Folding of `transcript.v1` events into view items and their rendering (design D3).
//!
//! [`Folder`] is fed the stored events in id order and reports which items were added or changed;
//! [`render_item`] is a pure function of an item. Everything in a transcript is untrusted: text is
//! escaped, Markdown is rendered without raw HTML.

use std::collections::HashMap;

use df_db::task_views::StoredEvent;
use df_proto::transcript::v1::{
    ContentBlock, NoticeLevel, PlanEntryStatus, StopReason, ToolCallContent, ToolCallStatus,
    ToolKind, content_block, event, tool_call_content,
};
use maud::{Markup, PreEscaped, html};

use crate::components::transcript as c;

/// A rendered element of the transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// DOM id; stable for the lifetime of the item so it can be morphed in place.
    pub dom_id: String,
    turn_id: String,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    User {
        prompt_id: String,
        author: String,
        text: String,
    },
    Agent {
        text: String,
        complete: bool,
    },
    Thought {
        text: String,
    },
    Tool(ToolItem),
    Plan(Vec<PlanItem>),
    TurnEnd {
        stop_reason: String,
        detail: String,
        usage: Vec<UsageItem>,
    },
    Notice {
        level: String,
        code: String,
        message: String,
    },
    Unreadable,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolItem {
    pub title: String,
    pub kind: String,
    pub status: String,
    pub content: Vec<ToolContent>,
    pub locations: Vec<String>,
    pub raw_input: String,
    pub raw_output: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolContent {
    Text(String),
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanItem {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageItem {
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// What an event did to the folded transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Index into [`Folder::items`].
    pub index: usize,
    /// `true` if the item was added, `false` if an existing one changed.
    pub added: bool,
}

/// Folds events into items.
#[derive(Debug, Default)]
pub struct Folder {
    items: Vec<Item>,
    /// Agent messages and thoughts by their DOM id.
    streams: HashMap<String, usize>,
    /// Tool calls by turn and call id.
    tools: HashMap<(String, String), usize>,
    /// The plan of a turn.
    plans: HashMap<String, usize>,
    /// Id of the last event folded.
    last_id: i64,
}

impl Folder {
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Id of the last event folded, 0 if none.
    pub fn last_id(&self) -> i64 {
        self.last_id
    }

    /// Folds one event. Returns the touched items in order.
    pub fn apply(&mut self, stored: &StoredEvent) -> Vec<Change> {
        self.last_id = self.last_id.max(stored.id);
        let Some(event) = &stored.event else {
            return vec![self.push(
                format!("ev-{}", stored.id),
                String::new(),
                ItemKind::Unreadable,
            )];
        };
        let turn = event.turn_id.clone();
        let Some(kind) = &event.kind else {
            return Vec::new();
        };
        match kind {
            event::Kind::TurnStarted(_) => Vec::new(),
            event::Kind::UserMessage(m) => {
                let author = m.author.as_ref().map_or_else(String::new, author_name);
                let text = blocks_text(&m.content);
                vec![self.push(
                    format!("ev-{}", stored.id),
                    turn,
                    ItemKind::User {
                        prompt_id: m.prompt_id.clone(),
                        author,
                        text,
                    },
                )]
            }
            event::Kind::AgentMessageChunk(c) => {
                let text = c.content.as_ref().map(block_text).unwrap_or_default();
                let dom_id = format!("msg-{}", dom_safe(&c.message_id));
                vec![self.append_stream(dom_id, turn, &text, false)]
            }
            event::Kind::AgentThoughtChunk(c) => {
                let text = c.content.as_ref().map(block_text).unwrap_or_default();
                let dom_id = format!("thought-{}", dom_safe(&c.message_id));
                vec![self.append_stream(dom_id, turn, &text, true)]
            }
            event::Kind::ToolCall(t) => {
                vec![self.upsert_tool(&turn, &t.tool_call_id, stored.id, |item| {
                    item.title.clone_from(&t.title);
                    item.kind = tool_kind(t.kind);
                    item.status = tool_status(t.status);
                    item.content = t.content.iter().filter_map(tool_content).collect();
                    item.locations = t.locations.iter().map(location).collect();
                    item.raw_input.clone_from(&t.raw_input_json);
                    item.raw_output.clone_from(&t.raw_output_json);
                    item.truncated = t.truncated;
                })]
            }
            event::Kind::ToolCallUpdate(u) => {
                vec![self.upsert_tool(&turn, &u.tool_call_id, stored.id, |item| {
                    if let Some(title) = &u.title {
                        item.title.clone_from(title);
                    }
                    if let Some(kind) = u.kind {
                        item.kind = tool_kind(kind);
                    }
                    if let Some(status) = u.status {
                        item.status = tool_status(status);
                    }
                    if let Some(content) = &u.content {
                        item.content = content.items.iter().filter_map(tool_content).collect();
                    }
                    if let Some(locations) = &u.locations {
                        item.locations = locations.items.iter().map(location).collect();
                    }
                    if let Some(raw) = &u.raw_input_json {
                        item.raw_input.clone_from(raw);
                    }
                    if let Some(raw) = &u.raw_output_json {
                        item.raw_output.clone_from(raw);
                    }
                    if let Some(truncated) = u.truncated {
                        item.truncated = truncated;
                    }
                })]
            }
            event::Kind::Plan(p) => {
                let entries = p
                    .entries
                    .iter()
                    .map(|e| PlanItem {
                        content: e.content.clone(),
                        status: PlanEntryStatus::try_from(e.status)
                            .map_or("unspecified", |s| plan_status(s))
                            .to_string(),
                    })
                    .collect();
                match self.plans.get(&turn) {
                    Some(&index) => {
                        // `plans` only holds indices of pushed items, which are never removed.
                        self.items[index].kind = ItemKind::Plan(entries);
                        vec![Change {
                            index,
                            added: false,
                        }]
                    }
                    None => {
                        let change = self.push(
                            format!("ev-{}", stored.id),
                            turn.clone(),
                            ItemKind::Plan(entries),
                        );
                        self.plans.insert(turn, change.index);
                        vec![change]
                    }
                }
            }
            event::Kind::TurnEnded(t) => {
                // The turn is over: its agent messages are complete and get rendered as Markdown.
                let mut changes: Vec<Change> = Vec::new();
                for (index, item) in self.items.iter_mut().enumerate() {
                    if item.turn_id == turn
                        && let ItemKind::Agent { complete, .. } = &mut item.kind
                        && !*complete
                    {
                        *complete = true;
                        changes.push(Change {
                            index,
                            added: false,
                        });
                    }
                }
                let usage = t
                    .usage
                    .iter()
                    .map(|u| UsageItem {
                        model: u.model_id.clone(),
                        input: u.input_tokens,
                        output: u.output_tokens,
                        cache_read: u.cache_read_tokens,
                        cache_write: u.cache_write_tokens,
                    })
                    .collect();
                changes.push(self.push(
                    format!("ev-{}", stored.id),
                    turn,
                    ItemKind::TurnEnd {
                        stop_reason: stop_reason(t.stop_reason),
                        detail: t.detail.clone(),
                        usage,
                    },
                ));
                changes
            }
            event::Kind::Notice(n) => vec![self.push(
                format!("ev-{}", stored.id),
                turn,
                ItemKind::Notice {
                    level: notice_level(n.level),
                    code: n.code.clone(),
                    message: n.message.clone(),
                },
            )],
        }
    }

    fn push(&mut self, dom_id: String, turn_id: String, kind: ItemKind) -> Change {
        self.items.push(Item {
            dom_id,
            turn_id,
            kind,
        });
        Change {
            index: self.items.len() - 1,
            added: true,
        }
    }

    fn append_stream(&mut self, dom_id: String, turn: String, text: &str, thought: bool) -> Change {
        if let Some(&index) = self.streams.get(&dom_id) {
            if let Some(ItemKind::Agent { text: t, .. } | ItemKind::Thought { text: t }) =
                self.items.get_mut(index).map(|item| &mut item.kind)
            {
                t.push_str(text);
            }
            return Change {
                index,
                added: false,
            };
        }
        let kind = if thought {
            ItemKind::Thought {
                text: text.to_string(),
            }
        } else {
            ItemKind::Agent {
                text: text.to_string(),
                complete: false,
            }
        };
        let change = self.push(dom_id.clone(), turn, kind);
        self.streams.insert(dom_id, change.index);
        change
    }

    fn upsert_tool(
        &mut self,
        turn: &str,
        call_id: &str,
        event_id: i64,
        update: impl FnOnce(&mut ToolItem),
    ) -> Change {
        let key = (turn.to_string(), call_id.to_string());
        if let Some(&index) = self.tools.get(&key)
            && let Some(ItemKind::Tool(item)) = self.items.get_mut(index).map(|i| &mut i.kind)
        {
            update(item);
            return Change {
                index,
                added: false,
            };
        }
        let mut item = ToolItem::default();
        update(&mut item);
        let change = self.push(
            format!("ev-{event_id}"),
            turn.to_string(),
            ItemKind::Tool(item),
        );
        self.tools.insert(key, change.index);
        change
    }
}

/// Restricts an id coming from the runtime to characters that are safe in an HTML id and in the
/// CSS selector htmx targets it with.
fn dom_safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn author_name(p: &df_proto::transcript::v1::Principal) -> String {
    use df_proto::transcript::v1::principal::Kind;
    match &p.kind {
        Some(Kind::Human(h)) => h.display_name.clone(),
        Some(Kind::Agent(a)) => a.display_name.clone(),
        Some(Kind::System(s)) => s.reason.clone(),
        None => String::new(),
    }
}

pub(crate) fn blocks_text(blocks: &[ContentBlock]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

fn block_text(block: &ContentBlock) -> String {
    match &block.kind {
        Some(content_block::Kind::Text(t)) => t.text.clone(),
        Some(content_block::Kind::Image(i)) => format!("[image {}]", i.mime_type),
        Some(content_block::Kind::ResourceLink(l)) => format!("[{}]({})", l.name, l.uri),
        Some(content_block::Kind::Resource(r)) => match &r.contents {
            Some(df_proto::transcript::v1::embedded_resource::Contents::Text(t)) => t.clone(),
            _ => format!("[resource {}]", r.uri),
        },
        None => String::new(),
    }
}

fn tool_content(c: &ToolCallContent) -> Option<ToolContent> {
    match c.kind.as_ref()? {
        tool_call_content::Kind::Content(block) => Some(ToolContent::Text(block_text(block))),
        tool_call_content::Kind::Diff(d) => Some(ToolContent::Diff {
            path: d.path.clone(),
            old: d.old_text.clone(),
            new: d.new_text.clone(),
        }),
    }
}

fn location(l: &df_proto::transcript::v1::Location) -> String {
    match l.line {
        Some(line) => format!("{}:{line}", l.path),
        None => l.path.clone(),
    }
}

fn tool_kind(kind: i32) -> String {
    ToolKind::try_from(kind).map_or_else(
        |_| "other".into(),
        |k| {
            k.as_str_name()
                .trim_start_matches("TOOL_KIND_")
                .to_lowercase()
        },
    )
}

fn tool_status(status: i32) -> String {
    ToolCallStatus::try_from(status).map_or_else(
        |_| "unknown".into(),
        |s| {
            s.as_str_name()
                .trim_start_matches("TOOL_CALL_STATUS_")
                .to_lowercase()
        },
    )
}

fn plan_status(status: PlanEntryStatus) -> &'static str {
    match status {
        PlanEntryStatus::Pending => "pending",
        PlanEntryStatus::InProgress => "in progress",
        PlanEntryStatus::Completed => "completed",
        PlanEntryStatus::Unspecified => "unspecified",
    }
}

fn stop_reason(reason: i32) -> String {
    StopReason::try_from(reason).map_or_else(
        |_| "unknown".into(),
        |r| {
            r.as_str_name()
                .trim_start_matches("STOP_REASON_")
                .to_lowercase()
        },
    )
}

fn notice_level(level: i32) -> String {
    NoticeLevel::try_from(level).map_or_else(
        |_| "info".into(),
        |l| {
            l.as_str_name()
                .trim_start_matches("NOTICE_LEVEL_")
                .to_lowercase()
        },
    )
}

/// Renders Markdown without raw HTML: markup in the source is escaped, unsafe link schemes are
/// dropped.
pub fn markdown(source: &str) -> Markup {
    let mut options = comrak::Options::default();
    options.render.escape = true;
    options.render.r#unsafe = false;
    PreEscaped(comrak::markdown_to_html(source, &options))
}

/// Renders the transcript container with all `items`.
pub fn render_transcript(items: &[Item]) -> Markup {
    html! {
        div id="transcript" class="flex min-w-0 flex-col gap-3" {
            @for item in items { (render_item(item)) }
        }
    }
}

/// Renders one item. The outer element carries the item's DOM id.
pub fn render_item(item: &Item) -> Markup {
    let id = item.dom_id.as_str();
    match &item.kind {
        ItemKind::User {
            prompt_id,
            author,
            text,
        } => c::user_message(id, prompt_id, author, text),
        ItemKind::Agent { text, complete } => {
            let body = if *complete {
                markdown(text)
            } else {
                html! { (text) }
            };
            c::agent_message(id, body, *complete)
        }
        ItemKind::Thought { text } => c::thought(id, text),
        ItemKind::Tool(tool) => render_tool(id, tool),
        ItemKind::Plan(entries) => {
            let entries: Vec<(String, String)> = entries
                .iter()
                .map(|e| (e.status.clone(), e.content.clone()))
                .collect();
            c::plan(id, &entries)
        }
        ItemKind::TurnEnd {
            stop_reason,
            detail,
            usage,
        } => {
            let lines: Vec<String> = usage
                .iter()
                .map(|u| {
                    format!(
                        "{}: {} in, {} out, {} cache read, {} cache write",
                        u.model, u.input, u.output, u.cache_read, u.cache_write
                    )
                })
                .collect();
            c::turn_end(id, stop_reason, detail, &lines)
        }
        ItemKind::Notice {
            level,
            code,
            message,
        } => c::notice(id, level, code, message),
        ItemKind::Unreadable => c::unreadable(id),
    }
}

fn render_tool(id: &str, tool: &ToolItem) -> Markup {
    let mut blocks: Vec<(String, String)> = Vec::new();
    if !tool.raw_input.is_empty() {
        blocks.push(("Input".into(), tool.raw_input.clone()));
    }
    for content in &tool.content {
        blocks.push(match content {
            ToolContent::Text(text) => ("Output".into(), text.clone()),
            ToolContent::Diff { path, old, new } => (path.clone(), diff_text(old.as_deref(), new)),
        });
    }
    if !tool.raw_output.is_empty() {
        blocks.push(("Result".into(), tool.raw_output.clone()));
    }
    c::tool_row(&c::ToolRow {
        id,
        title: &tool.title,
        kind: &tool.kind,
        status: &tool.status,
        locations: &tool.locations,
        blocks: &blocks,
        truncated: tool.truncated,
    })
}

fn diff_text(old: Option<&str>, new: &str) -> String {
    let removed = old
        .into_iter()
        .flat_map(str::lines)
        .map(|l| format!("-{l}\n"));
    let added = new.lines().map(|l| format!("+{l}\n"));
    removed.chain(added).collect()
}

#[cfg(test)]
mod tests;
