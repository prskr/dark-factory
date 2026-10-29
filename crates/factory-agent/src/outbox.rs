//! Durable outbox implementation according to Design D3.
//!
//! Segment files are stored under `<data_dir>/runtime/outbox/` with length-prefixed
//! protobuf `AgentMessage`s. Sequence numbers start at 1 per run and strictly increase.
//! Acknowledged segments are deleted once all messages in the segment have been persisted.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use df_proto::agent::v1::AgentMessage;
use prost::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, watch};

pub const DEFAULT_MAX_OUTBOX_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB
pub const DEFAULT_MAX_SEGMENT_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunState {
    pub run_id: String,
    pub next_seq: u64,
    #[serde(default)]
    pub last_pushed_sha: Option<String>,
    #[serde(default)]
    pub harness_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SegmentInfo {
    path: PathBuf,
    last_seq: u64,
    size_bytes: u64,
}

struct ActiveSegment {
    file: BufWriter<File>,
    path: PathBuf,
    last_seq: u64,
    bytes_written: u64,
}

pub struct Outbox {
    dir: PathBuf,
    run_file_path: PathBuf,
    run_state: RunState,
    max_bytes: u64,
    max_segment_bytes: u64,
    active_segment: Option<ActiveSegment>,
    closed_segments: Vec<SegmentInfo>,
    total_bytes: u64,
    space_available: Arc<Notify>,
}

impl Outbox {
    /// Open or initialize an outbox in `<runtime_dir>/outbox`.
    ///
    /// A non-empty `run_id` that differs from the stored one discards the stored state. An empty
    /// `run_id` keeps whatever is stored, so the caller can decide once it learns the run id.
    pub fn open(
        runtime_dir: &Path,
        run_id: &str,
        max_bytes: u64,
        max_segment_bytes: u64,
    ) -> anyhow::Result<Self> {
        let outbox_dir = runtime_dir.join("outbox");
        std::fs::create_dir_all(&outbox_dir)?;
        let run_file_path = runtime_dir.join("run.json");

        let mut run_state = if run_file_path.exists() {
            let data = std::fs::read_to_string(&run_file_path)?;
            serde_json::from_str::<RunState>(&data).unwrap_or_else(|e| {
                tracing::warn!(
                    path = %run_file_path.display(),
                    error = %e,
                    "run.json is corrupt, resetting run state"
                );
                RunState {
                    run_id: run_id.to_string(),
                    next_seq: 1,
                    last_pushed_sha: None,
                    harness_session_id: None,
                }
            })
        } else {
            RunState {
                run_id: run_id.to_string(),
                next_seq: 1,
                last_pushed_sha: None,
                harness_session_id: None,
            }
        };

        // Scan existing segments once on open to initialize total_bytes and closed_segments (Finding 27)
        let mut total_bytes = 0;
        let mut closed_segments = Vec::new();
        if outbox_dir.exists() {
            for entry in std::fs::read_dir(&outbox_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "log") {
                    let len = entry.metadata()?.len();
                    total_bytes += len;
                    let msgs = Self::read_segment_file(&path).unwrap_or_default();
                    let last_seq = msgs.iter().map(|m| m.seq).max().unwrap_or(0);
                    closed_segments.push(SegmentInfo {
                        path,
                        last_seq,
                        size_bytes: len,
                    });
                }
            }
        }
        closed_segments.sort_by(|a, b| a.path.cmp(&b.path));

        let mut outbox = Self {
            dir: outbox_dir,
            run_file_path,
            run_state: run_state.clone(),
            max_bytes,
            max_segment_bytes,
            active_segment: None,
            closed_segments,
            total_bytes,
            space_available: Arc::new(Notify::new()),
        };

        // If run_id differs from stored run_id, discard leftover outbox
        if !run_id.is_empty() && run_state.run_id != run_id {
            outbox.discard(run_id)?;
        } else {
            // Find highest seq across existing segments to avoid sequence reuse
            let highest_seq = outbox
                .closed_segments
                .iter()
                .map(|s| s.last_seq)
                .max()
                .unwrap_or(0);
            if highest_seq >= run_state.next_seq {
                run_state.next_seq = highest_seq + 1;
                outbox.run_state = run_state;
            }
            // Persist the state right away: appends don't write `run.json`, so without this a
            // reopen could not tell which run the segments on disk belong to.
            outbox.save_run_state()?;
        }

        Ok(outbox)
    }

    pub fn run_state(&self) -> &RunState {
        &self.run_state
    }

    pub fn set_harness_session_id(&mut self, session_id: Option<String>) -> anyhow::Result<()> {
        self.run_state.harness_session_id = session_id;
        self.save_run_state()
    }

    pub fn set_last_pushed_sha(&mut self, sha: Option<String>) -> anyhow::Result<()> {
        self.run_state.last_pushed_sha = sha;
        self.save_run_state()
    }

    /// Atomically persists `run.json` by writing to a temporary file, syncing, and renaming (Finding 25).
    pub fn save_run_state(&self) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(&self.run_state)?;
        let tmp_path = self.run_file_path.with_extension("tmp");
        {
            let mut file = File::create(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &self.run_file_path)?;
        // Make the rename durable
        if let Some(parent) = self.run_file_path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    /// True if every stored message has been acknowledged.
    pub fn is_empty(&self) -> bool {
        self.total_bytes == 0
    }

    /// Discard all existing segment files and reset run state with a new run id.
    pub fn discard(&mut self, new_run_id: &str) -> anyhow::Result<()> {
        self.active_segment = None;
        self.closed_segments.clear();
        self.total_bytes = 0;
        if self.dir.exists() {
            for entry in std::fs::read_dir(&self.dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    std::fs::remove_file(entry.path()).ok(); // best-effort cleanup
                }
            }
        }
        self.run_state = RunState {
            run_id: new_run_id.to_string(),
            next_seq: 1,
            last_pushed_sha: None,
            harness_session_id: None,
        };
        self.save_run_state()?;
        self.space_available.notify_waiters();
        Ok(())
    }

    /// Append an `AgentMessage` to the outbox. Assigns sequence number, writes length-delimited
    /// protobuf to the active segment file, and flushes to disk.
    ///
    /// Appends a message to the outbox without awaiting backpressure.
    /// For backpressure handling without lock deadlocks, use `append_with_backpressure`.
    pub fn append(&mut self, mut msg: AgentMessage) -> anyhow::Result<u64> {
        let mut encoded = Vec::new();
        let seq = self.run_state.next_seq;
        msg.seq = seq;
        msg.encode_length_delimited(&mut encoded)?;
        let msg_len = encoded.len() as u64;

        self.append_encoded(seq, encoded, msg_len)
    }

    /// Appends an already-encoded message and advances sequence and `total_bytes`.
    fn append_encoded(&mut self, seq: u64, encoded: Vec<u8>, msg_len: u64) -> anyhow::Result<u64> {
        // Check if we need to rollover active segment
        let need_new_segment = match &self.active_segment {
            Some(seg) => seg.bytes_written + msg_len > self.max_segment_bytes,
            None => true,
        };

        if need_new_segment {
            self.rollover_segment(seq)?;
        }

        // Write message to active segment
        let seg = self.active_segment.as_mut().expect("active segment exists");
        seg.file.write_all(&encoded)?;
        seg.file.flush()?;
        seg.file.get_ref().sync_data()?;
        seg.bytes_written += msg_len;
        seg.last_seq = seq;
        self.total_bytes += msg_len;

        // Advance the sequence number. `run.json` is saved when segments are acknowledged;
        // until then `open` recovers `next_seq` from the segments on disk.
        self.run_state.next_seq = seq + 1;

        Ok(seq)
    }

    /// Asynchronously appends a message while applying back-pressure WITHOUT holding the outbox lock,
    /// preventing connection manager deadlocks (Finding 2).
    pub async fn append_with_backpressure(
        outbox_arc: &Arc<tokio::sync::Mutex<Outbox>>,
        msg: AgentMessage,
    ) -> anyhow::Result<u64> {
        Self::append_inner(outbox_arc, msg, None).await
    }

    /// Like [`Self::append_with_backpressure`], but stops waiting for free space once `stop`
    /// is set and appends anyway. The size limit is a soft cap, so a message that was already
    /// taken off the outbound channel is never lost.
    pub async fn append_or_stop(
        outbox_arc: &Arc<tokio::sync::Mutex<Outbox>>,
        msg: AgentMessage,
        stop: &mut watch::Receiver<bool>,
    ) -> anyhow::Result<u64> {
        Self::append_inner(outbox_arc, msg, Some(stop)).await
    }

    async fn append_inner(
        outbox_arc: &Arc<tokio::sync::Mutex<Outbox>>,
        mut msg: AgentMessage,
        mut stop: Option<&mut watch::Receiver<bool>>,
    ) -> anyhow::Result<u64> {
        // Upper bound of the encoded size (the `seq` field adds at most 11 bytes)
        let body_len = msg.encoded_len();
        let msg_len = (body_len + prost::length_delimiter_len(body_len) + 11) as u64;

        let notify = outbox_arc.lock().await.space_available.clone();
        loop {
            // Register for wakeups before checking, so an ack in between is not missed
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let is_full = {
                let ob = outbox_arc.lock().await;
                // An empty outbox always accepts, so an oversized message cannot wait forever
                ob.total_bytes > 0 && ob.total_bytes + msg_len > ob.max_bytes
            };
            if !is_full || stop.as_ref().is_some_and(|rx| *rx.borrow()) {
                break;
            }
            tokio::select! {
                () = &mut notified => {}
                () = wait_stop(&mut stop) => break,
            }
        }

        let mut ob = outbox_arc.lock().await;
        let seq = ob.run_state.next_seq;
        let mut encoded = Vec::new();
        msg.seq = seq;
        msg.encode_length_delimited(&mut encoded)?;
        let len = encoded.len() as u64;
        ob.append_encoded(seq, encoded, len)
    }

    fn rollover_segment(&mut self, first_seq: u64) -> anyhow::Result<()> {
        if let Some(mut seg) = self.active_segment.take() {
            seg.file.flush()?;
            seg.file.get_ref().sync_all()?;
            self.closed_segments.push(SegmentInfo {
                path: seg.path,
                last_seq: seg.last_seq,
                size_bytes: seg.bytes_written,
            });
        }

        let seg_name = format!("seg_{first_seq:016x}.log");
        let path = self.dir.join(seg_name);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        self.active_segment = Some(ActiveSegment {
            file: BufWriter::new(file),
            path,
            last_seq: first_seq,
            bytes_written: 0,
        });

        Ok(())
    }

    /// Trims acknowledged messages: deletes segment files whose messages are all `<= ack_seq`.
    pub fn acknowledge(&mut self, ack_seq: u64) -> anyhow::Result<()> {
        // Flush active segment first
        if let Some(seg) = self.active_segment.as_mut() {
            seg.file.flush()?;
            seg.file.get_ref().sync_all()?;
        }

        let mut to_delete: Vec<(PathBuf, u64)> = Vec::new();
        self.closed_segments.retain(|seg| {
            let acked = seg.last_seq <= ack_seq;
            if acked {
                to_delete.push((seg.path.clone(), seg.size_bytes));
            }
            !acked
        });
        if let Some(active) = &self.active_segment
            && active.last_seq <= ack_seq
        {
            to_delete.push((active.path.clone(), active.bytes_written));
            self.active_segment = None;
        }

        if to_delete.is_empty() {
            return Ok(());
        }

        // Persist `next_seq` BEFORE the segments that carry it disappear, so a crash in between
        // can't make a restart reuse seqs the control plane already persisted.
        self.save_run_state()?;
        for (path, bytes) in to_delete {
            std::fs::remove_file(path).ok(); // best-effort cleanup
            self.total_bytes = self.total_bytes.saturating_sub(bytes);
        }

        self.space_available.notify_waiters();
        Ok(())
    }

    /// Read all unacknowledged messages across all segments with `seq > last_persisted_seq`.
    pub fn read_unacknowledged(
        &self,
        last_persisted_seq: u64,
    ) -> anyhow::Result<Vec<AgentMessage>> {
        let segment_paths = self.list_segment_files()?;
        let mut unacked = Vec::new();

        for path in segment_paths {
            let messages = Self::read_segment_file(&path)?;
            for msg in messages {
                if msg.seq > last_persisted_seq {
                    unacked.push(msg);
                }
            }
        }

        unacked.sort_by_key(|m| m.seq);
        Ok(unacked)
    }

    fn list_segment_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        if self.dir.exists() {
            for entry in std::fs::read_dir(&self.dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|ext| ext == "log") {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn read_segment_file(path: &Path) -> anyhow::Result<Vec<AgentMessage>> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let mut messages = Vec::new();
        let mut cursor = Cursor::new(&buffer);

        while usize::try_from(cursor.position()).is_ok_and(|pos| pos < buffer.len()) {
            match AgentMessage::decode_length_delimited(&mut cursor) {
                Ok(msg) => messages.push(msg),
                Err(_) => {
                    // Truncated trailing record at EOF (e.g. process was killed midway)
                    break;
                }
            }
        }

        Ok(messages)
    }
}

/// Resolves when `stop` is set (or its sender is gone); pending forever without a receiver.
async fn wait_stop(stop: &mut Option<&mut watch::Receiver<bool>>) {
    match stop {
        Some(rx) => {
            rx.wait_for(|stopped| *stopped).await.ok();
        }
        None => std::future::pending::<()>().await,
    }
}
