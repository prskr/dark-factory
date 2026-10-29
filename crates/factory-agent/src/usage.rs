//! Claude Code session log parsing and usage tracking.
//!
//! Follows `design/agent-run.md` §5.2, design D5, and `specs/acp-harness/spec.md`.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use df_proto::transcript::v1::{Usage, UsageSource};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SessionLogEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    id: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<SessionLogMessage>,
}

#[derive(Debug, Deserialize)]
struct SessionLogMessage {
    id: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    model: Option<String>,
    role: Option<String>,
    usage: Option<SessionLogUsage>,
}

#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "the field names are those of the JSON log"
)]
struct SessionLogUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Convert a directory path to Claude Code project directory slug.
///
/// For example, `/data/repo` becomes `-data-repo`.
pub fn cwd_to_slug(cwd: &Path) -> String {
    let s = cwd.to_string_lossy();
    let mut slug = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '/' || c == '.' {
            slug.push('-');
        } else {
            slug.push(c);
        }
    }
    slug
}

/// Parses session log JSONL entries from a reader and returns aggregated `Usage` per model.
pub fn parse_session_log(reader: impl BufRead) -> std::io::Result<Vec<Usage>> {
    let mut usage_by_model: HashMap<String, Usage> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<SessionLogEntry>(trimmed) {
            let is_assistant = entry.entry_type.as_deref() == Some("assistant")
                || entry.message.as_ref().and_then(|m| m.role.as_deref()) == Some("assistant");

            if is_assistant
                && let Some(msg) = entry.message
                && let Some(raw_usage) = msg.usage
            {
                // Deduplicate by message id or request id if available (Finding 21)
                let msg_id = msg
                    .id
                    .as_ref()
                    .or(msg.request_id.as_ref())
                    .or(entry.id.as_ref())
                    .or(entry.request_id.as_ref());

                if let Some(id) = msg_id
                    && !seen_ids.insert(id.clone())
                {
                    continue;
                }

                let model = msg.model.unwrap_or_else(|| "unknown".to_string());
                let u = usage_by_model
                    .entry(model.clone())
                    .or_insert_with(|| Usage {
                        model_id: model,
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                    });
                u.input_tokens += raw_usage.input_tokens;
                u.output_tokens += raw_usage.output_tokens;
                u.cache_read_tokens += raw_usage.cache_read_input_tokens;
                u.cache_write_tokens += raw_usage.cache_creation_input_tokens;
            }
        }
    }

    let mut result: Vec<Usage> = usage_by_model.into_values().collect();
    result.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    Ok(result)
}

/// Tracks byte offset in Claude Code session log across turns to capture per-turn usage.
#[derive(Debug)]
pub struct SessionLogTracker {
    home_dir: PathBuf,
    cwd: PathBuf,
    session_id: String,
    current_offset: u64,
    /// True once `mark_turn_start` skipped what was in the file before the first turn.
    offset_initialized: bool,
}

impl SessionLogTracker {
    /// Create a new session log tracker for the given home directory, cwd, and session ID.
    pub fn new(
        home_dir: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            home_dir: home_dir.into(),
            cwd: cwd.into(),
            session_id: session_id.into(),
            current_offset: 0,
            offset_initialized: false,
        }
    }

    /// Finds the path to the session JSONL file.
    pub fn session_file_path(&self) -> PathBuf {
        let slug = cwd_to_slug(&self.cwd);
        let direct = self
            .home_dir
            .join(".claude")
            .join("projects")
            .join(&slug)
            .join(format!("{}.jsonl", self.session_id));

        if direct.exists() {
            return direct;
        }

        // Fallback: search all directories under projects/ for <session_id>.jsonl
        let projects_dir = self.home_dir.join(".claude").join("projects");
        std::fs::read_dir(&projects_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
            .map(|entry| entry.path().join(format!("{}.jsonl", self.session_id)))
            .find(|candidate| candidate.exists())
            .unwrap_or(direct)
    }

    /// Mark the beginning of a turn.
    ///
    /// On the first call, sets the offset to the current file end so history from earlier runs is
    /// not counted. Later calls keep the offset: it already points behind the last complete line
    /// that `parse_turn_usage` read, and records flushed after a turn ended are counted later
    /// instead of being skipped.
    pub fn mark_turn_start(&mut self) {
        if self.offset_initialized {
            return;
        }
        self.offset_initialized = true;
        self.current_offset =
            std::fs::metadata(self.session_file_path()).map_or(0, |meta| meta.len());
    }

    /// Parse token usage written since the last turn start or offset.
    ///
    /// Updates `current_offset` to the end of the last complete (newline-terminated) line, so a
    /// record that is still being written is read on the next call.
    pub fn parse_turn_usage(&mut self) -> (Vec<Usage>, UsageSource) {
        let path = self.session_file_path();
        let Ok(file) = File::open(&path) else {
            return (Vec::new(), UsageSource::Unspecified);
        };

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.current_offset)).is_err() {
            return (Vec::new(), UsageSource::Unspecified);
        }
        let mut buf = Vec::new();
        if reader.read_to_end(&mut buf).is_err() {
            return (Vec::new(), UsageSource::Unspecified);
        }

        let complete_len = buf.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let usage =
            parse_session_log(buf.get(..complete_len).unwrap_or_default()).unwrap_or_default();
        self.current_offset += complete_len as u64;

        if usage.is_empty() {
            (Vec::new(), UsageSource::Unspecified)
        } else {
            (usage, UsageSource::SessionLog)
        }
    }
}
