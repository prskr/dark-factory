//! Folding of `transcript.v1` events into view items and their rendering (design D3).
//!
//! [`Folder`] is fed the stored events in id order and reports which items were added or changed;
//! [`render_item`] is a pure function of an item. Everything in a transcript is untrusted: text is
//! escaped, Markdown is rendered without raw HTML.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use df_db::task_views::StoredEvent;
use df_proto::transcript::v1::{
    ContentBlock, NoticeLevel, Plan, PlanEntryStatus, ToolCall, ToolCallContent, ToolCallStatus,
    ToolCallUpdate, ToolKind, UserMessage, content_block, event, tool_call_content,
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
    /// Who wrote the item (board member or the persona), empty for items without an author.
    pub author: String,
    /// When the event that created the item was recorded.
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    User {
        prompt_id: String,
        author: String,
        /// Text parts as `(text, hidden)`; hidden parts are only present with
        /// `web.showHiddenBlocks`.
        parts: Vec<(String, bool)>,
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
    Notice {
        level: String,
        code: String,
        message: String,
    },
    Unreadable,
    /// The start of a run; precedes the run's first item.
    RunStart {
        number: i64,
        started_at: DateTime<Utc>,
    },
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
    /// Name of the persona, the author of agent items.
    agent_name: String,
    /// Run of the last event folded, 0 before the first.
    run: i64,
    /// A run that started and has no item yet: its divider goes in front of the next item.
    run_start: Option<(i64, DateTime<Utc>)>,
    /// Time of the event being folded.
    now: DateTime<Utc>,
    /// Changes of dividers pushed while folding the current event.
    carry: Vec<Change>,
    /// Keep hidden content blocks in user messages (`web.showHiddenBlocks`).
    show_hidden: bool,
}

impl Folder {
    /// A folder whose agent items are authored by `agent_name`.
    pub fn new(agent_name: impl Into<String>) -> Self {
        Self {
            agent_name: agent_name.into(),
            ..Self::default()
        }
    }

    /// Keeps hidden content blocks of user messages, for debugging prompts.
    #[must_use]
    pub fn with_hidden_blocks(mut self, show_hidden: bool) -> Self {
        self.show_hidden = show_hidden;
        self
    }

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
        self.now = stored.occurred_at;
        if stored.run_number != self.run {
            self.run = stored.run_number;
            self.run_start = Some((stored.run_number, stored.run_started_at));
        }
        let changes = self.apply_event(stored);
        // Dividers pushed on the way come first.
        let mut all = std::mem::take(&mut self.carry);
        all.extend(changes);
        all
    }

    fn apply_event(&mut self, stored: &StoredEvent) -> Vec<Change> {
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
        let id = stored.id;
        match kind {
            event::Kind::TurnStarted(_) => Vec::new(),
            event::Kind::UserMessage(m) => self.apply_user_message(id, turn, m),
            event::Kind::AgentMessageChunk(c) => {
                let text = c.content.as_ref().map(block_text).unwrap_or_default();
                // Message ids restart with every run.
                let dom_id = format!("msg-r{}-{}", self.run, dom_safe(&c.message_id));
                vec![self.append_stream(dom_id, turn, &text, false)]
            }
            event::Kind::AgentThoughtChunk(c) => {
                let text = c.content.as_ref().map(block_text).unwrap_or_default();
                let dom_id = format!("thought-r{}-{}", self.run, dom_safe(&c.message_id));
                vec![self.append_stream(dom_id, turn, &text, true)]
            }
            event::Kind::ToolCall(t) => vec![self.apply_tool_call(id, &turn, t)],
            event::Kind::ToolCallUpdate(u) => vec![self.apply_tool_call_update(id, &turn, u)],
            event::Kind::Plan(p) => self.apply_plan(id, turn, p),
            event::Kind::TurnEnded(_) => self.complete_agent_messages(&turn),
            event::Kind::Notice(n) => vec![self.push(
                format!("ev-{id}"),
                turn,
                ItemKind::Notice {
                    level: notice_level(n.level),
                    code: n.code.clone(),
                    message: n.message.clone(),
                },
            )],
        }
    }

    /// A user message; one without visible parts adds nothing.
    fn apply_user_message(&mut self, id: i64, turn: String, m: &UserMessage) -> Vec<Change> {
        let author = m.author.as_ref().map_or_else(String::new, author_name);
        let parts = block_parts(&m.content, self.show_hidden);
        if parts.is_empty() {
            return Vec::new();
        }
        vec![self.push(
            format!("ev-{id}"),
            turn,
            ItemKind::User {
                prompt_id: m.prompt_id.clone(),
                author,
                parts,
            },
        )]
    }

    /// A tool call announcement: sets all fields of the tool item.
    fn apply_tool_call(&mut self, id: i64, turn: &str, t: &ToolCall) -> Change {
        self.upsert_tool(turn, &t.tool_call_id, id, |item| {
            item.title.clone_from(&t.title);
            item.kind = tool_kind(t.kind);
            item.status = tool_status(t.status);
            item.content = t.content.iter().filter_map(tool_content).collect();
            item.locations = t.locations.iter().map(location).collect();
            item.raw_input.clone_from(&t.raw_input_json);
            item.raw_output.clone_from(&t.raw_output_json);
            item.truncated = t.truncated;
        })
    }

    /// A tool call update: sets the fields it carries.
    fn apply_tool_call_update(&mut self, id: i64, turn: &str, u: &ToolCallUpdate) -> Change {
        self.upsert_tool(turn, &u.tool_call_id, id, |item| {
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
        })
    }

    /// A plan: one item per turn, replaced by later plans of the turn.
    fn apply_plan(&mut self, id: i64, turn: String, p: &Plan) -> Vec<Change> {
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
        // `plans` only holds indices of pushed items, which are never removed.
        let known = self
            .plans
            .get(&turn)
            .and_then(|&index| Some((index, self.items.get_mut(index)?)));
        if let Some((index, item)) = known {
            item.kind = ItemKind::Plan(entries);
            return vec![Change {
                index,
                added: false,
            }];
        }
        let change = self.push(format!("ev-{id}"), turn.clone(), ItemKind::Plan(entries));
        self.plans.insert(turn, change.index);
        vec![change]
    }

    /// The turn is over: its agent messages are complete and get rendered as Markdown.
    fn complete_agent_messages(&mut self, turn: &str) -> Vec<Change> {
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
        changes
    }

    fn push(&mut self, dom_id: String, turn_id: String, kind: ItemKind) -> Change {
        if let Some((number, started_at)) = self.run_start.take() {
            self.items.push(Item {
                dom_id: format!("run-{number}"),
                turn_id: String::new(),
                kind: ItemKind::RunStart { number, started_at },
                author: String::new(),
                at: started_at,
            });
            self.carry.push(Change {
                index: self.items.len() - 1,
                added: true,
            });
        }
        let author = match &kind {
            ItemKind::User { author, .. } => author.clone(),
            ItemKind::Agent { .. } | ItemKind::Thought { .. } | ItemKind::Tool(_) => {
                self.agent_name.clone()
            }
            _ => String::new(),
        };
        self.items.push(Item {
            dom_id,
            turn_id,
            kind,
            author,
            at: self.now,
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

/// The text of `blocks` as `(text, hidden)`; hidden blocks only with `show_hidden`.
pub(crate) fn block_parts(blocks: &[ContentBlock], show_hidden: bool) -> Vec<(String, bool)> {
    blocks
        .iter()
        .filter(|b| show_hidden || !b.hidden)
        .map(|b| (block_text(b), b.hidden))
        .collect()
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
/// dropped. The text is untrusted, so the browser must not fetch anything by itself: images
/// become plain links and every link carries `rel="noopener noreferrer nofollow"`.
pub fn markdown(source: &str) -> Markup {
    let mut options = comrak::Options::default();
    options.render.escape = true;
    options.render.r#unsafe = false;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, source, &options);
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        if let comrak::nodes::NodeValue::Image(link) = &data.value {
            data.value = comrak::nodes::NodeValue::Link(link.clone());
        }
    }
    let mut html = String::new();
    comrak::format_html(root, &options, &mut html).expect("formatting into a String cannot fail");
    PreEscaped(html.replace("<a href=", "<a rel=\"noopener noreferrer nofollow\" href="))
}

/// Whether a request card raised at `card_at` goes before an item recorded at `item_at`; at the
/// same time the card goes first. Page and stream place cards with this one rule.
pub(crate) fn card_goes_before(card_at: DateTime<Utc>, item_at: DateTime<Utc>) -> bool {
    card_at <= item_at
}

/// Renders the transcript container with all `items`. `cards` (a time and the markup) are
/// merged in by time (see [`card_goes_before`]).
pub fn render_transcript(items: &[Item], cards: &[(DateTime<Utc>, Markup)]) -> Markup {
    let mut cards = cards.iter().peekable();
    html! {
        div id="transcript" class="flex min-w-0 flex-col gap-3" {
            @for item in items {
                @while let Some((_, card)) = cards.next_if(|(at, _)| card_goes_before(*at, item.at)) { (card) }
                (render_item(item))
            }
            @for (_, card) in cards { (card) }
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
            parts,
        } => c::user_message(id, prompt_id, author, item.at, parts),
        ItemKind::Agent { text, complete } => {
            let body = if *complete {
                markdown(text)
            } else {
                html! { (text) }
            };
            let author = if item.author.is_empty() {
                "Agent"
            } else {
                &item.author
            };
            c::agent_message(id, author, item.at, body, *complete)
        }
        ItemKind::RunStart { number, started_at } => c::run_divider(id, *number, *started_at),
        ItemKind::Thought { text } => c::thought(id, text),
        ItemKind::Tool(tool) => render_tool(id, tool),
        ItemKind::Plan(entries) => {
            let entries: Vec<(String, String)> = entries
                .iter()
                .map(|e| (e.status.clone(), e.content.clone()))
                .collect();
            c::plan(id, &entries)
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
impl Item {
    /// A completed agent message, for tests of the layout around items.
    pub(crate) fn test_agent(id: &str, at: DateTime<Utc>) -> Self {
        Self {
            dom_id: id.to_string(),
            turn_id: String::new(),
            kind: ItemKind::Agent {
                text: "x".to_string(),
                complete: true,
            },
            author: String::new(),
            at,
        }
    }
}

#[cfg(test)]
mod tests;
