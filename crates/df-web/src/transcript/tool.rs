//! The details of an opened tool call (design D5): typed blocks instead of raw text.

use maud::Markup;
use serde_json::Value;

use super::{ToolContent, ToolItem, diff_text};
use crate::components::transcript::ToolBlock;
use crate::markdown;

/// The blocks of `tool` in reading order: input, then output and diffs, then the raw result.
pub(super) fn blocks(tool: &ToolItem) -> Vec<ToolBlock> {
    let mut blocks = Vec::new();
    if !tool.raw_input.is_empty() {
        blocks.push(ToolBlock {
            label: "Input",
            caption: None,
            parts: json_parts(&tool.raw_input, true),
        });
    }
    // The output of a running call is still growing: Markdown only once it is final.
    let finished = tool.status.is_finished();
    for content in &tool.content {
        blocks.push(match content {
            ToolContent::Text(text) => ToolBlock {
                label: "Output",
                caption: None,
                parts: vec![if finished {
                    markdown::render(text)
                } else {
                    markdown::plain_code(text)
                }],
            },
            ToolContent::Diff { path, old, new } => ToolBlock {
                label: "Diff",
                caption: Some(path.clone()),
                parts: vec![markdown::highlight_by_extension(
                    "diff",
                    &diff_text(old.as_deref(), new),
                )],
            },
        });
    }
    if !tool.raw_output.is_empty() {
        blocks.push(ToolBlock {
            label: "Result",
            caption: None,
            parts: json_parts(&tool.raw_output, false),
        });
    }
    blocks
}

/// Pretty-printed, highlighted JSON; with `with_command` a top-level `command` string comes
/// first as a shell line. Text that is not JSON stays plain.
fn json_parts(raw: &str, with_command: bool) -> Vec<Markup> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return vec![markdown::plain_code(raw)];
    };
    let mut parts = Vec::new();
    if with_command && let Some(command) = value.get("command").and_then(Value::as_str) {
        parts.push(markdown::highlight_by_extension("sh", command));
    }
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_else(|_| raw.to_owned());
    parts.push(markdown::highlight_by_extension("json", &pretty));
    parts
}
