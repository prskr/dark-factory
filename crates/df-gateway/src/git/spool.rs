//! Spools a `git-receive-pack` request body to a temporary file (design D4).
//!
//! The push is checked before it is forwarded, so the body has to be kept
//! somewhere; a file keeps the gateway's memory bounded by the read buffer
//! instead of the push size.

use std::io::{self, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::combinators::BoxBody;
use hyper::body::{Body, Frame};
use tempfile::{NamedTempFile, TempPath};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Bytes of the body kept in memory for parsing the ref update commands.
pub const HEAD_LIMIT: usize = 64 * 1024;

/// The pkt-line flush that makes up the probe POST git sends before a chunked push.
const PROBE_BODY: &[u8] = b"0000";

/// Read buffer size when streaming the spooled body back out.
const READ_CHUNK: usize = 64 * 1024;

/// Longest wait for the next frame of a request body before the request is aborted.
pub const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Failure to spool a request body.
#[derive(Debug, thiserror::Error)]
pub enum SpoolError {
    /// The body is larger than the maximum push size.
    #[error("request body exceeds the maximum push size of {max_bytes} bytes")]
    TooLarge { max_bytes: u64 },
    /// No frame of the body arrived within the idle timeout.
    #[error("request body timed out")]
    TimedOut,
    /// Reading the request body failed.
    #[error("reading the request body failed: {0}")]
    Body(String),
    /// Writing the temporary file failed.
    #[error("writing the spool file failed: {0}")]
    Io(#[from] io::Error),
}

/// A body that could not be spooled, with what was read of it.
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct SpoolFailure {
    pub error: SpoolError,
    /// The first bytes of the body (at most [`HEAD_LIMIT`]).
    pub head: Vec<u8>,
    /// Bytes read before the failure.
    pub read: u64,
}

impl From<io::Error> for SpoolFailure {
    fn from(error: io::Error) -> Self {
        Self {
            error: SpoolError::Io(error),
            head: Vec::new(),
            read: 0,
        }
    }
}

/// A request body stored in a temporary file, removed on drop.
#[derive(Debug)]
pub struct Spooled {
    file: NamedTempFile,
    len: u64,
    head: Vec<u8>,
}

impl Spooled {
    /// Body length in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the body is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The first bytes of the body (at most [`HEAD_LIMIT`]), for command parsing.
    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// Whether the body is the probe POST git sends before a chunked push.
    pub fn is_probe(&self) -> bool {
        self.head == PROBE_BODY
    }

    /// A file positioned at `offset`, for reading the pack. Blocking: call it
    /// from `spawn_blocking` together with the code that reads the file.
    pub fn reader_at(&self, offset: u64) -> io::Result<std::fs::File> {
        let mut file = self.file.reopen()?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(file)
    }

    /// Streams the whole body for forwarding; the file is removed once the
    /// returned body is dropped.
    pub fn into_body(self) -> BoxBody<Bytes, io::Error> {
        let (file, path) = self.file.into_parts();
        let state = (tokio::fs::File::from_std(file), path);
        let stream = futures::stream::try_unfold(state, read_frame);
        http_body_util::StreamBody::new(stream).boxed()
    }
}

/// Streams `body` into a new temporary file in `dir`, failing with
/// [`SpoolError::TooLarge`] as soon as it exceeds `max_bytes` and with
/// [`SpoolError::TimedOut`] if no frame arrives within `idle`.
pub async fn spool<B>(
    body: B,
    dir: &Path,
    max_bytes: u64,
    idle: Duration,
) -> Result<Spooled, SpoolFailure>
where
    B: Body<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let dir = dir.to_path_buf();
    let named = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&dir)?;
        NamedTempFile::new_in(&dir)
    })
    .await
    .map_err(io::Error::other)??;
    let mut out = tokio::fs::File::from_std(named.reopen()?);
    let mut len: u64 = 0;
    let mut head = Vec::new();
    let mut body = std::pin::pin!(body);
    loop {
        let failed = |error, head: &mut Vec<u8>, read| SpoolFailure {
            error,
            head: std::mem::take(head),
            read,
        };
        let Ok(next) = tokio::time::timeout(idle, body.frame()).await else {
            return Err(failed(SpoolError::TimedOut, &mut head, len));
        };
        let Some(frame) = next else {
            break;
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return Err(failed(SpoolError::Body(error.to_string()), &mut head, len));
            }
        };
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let room = HEAD_LIMIT.saturating_sub(head.len());
        head.extend_from_slice(data.get(..room.min(data.len())).unwrap_or_default());
        len = len.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
        if len > max_bytes {
            return Err(failed(SpoolError::TooLarge { max_bytes }, &mut head, len));
        }
        if let Err(error) = out.write_all(&data).await {
            return Err(failed(SpoolError::Io(error), &mut head, len));
        }
    }
    out.flush().await?;
    Ok(Spooled {
        file: named,
        len,
        head,
    })
}

async fn read_frame(
    (mut file, path): (tokio::fs::File, TempPath),
) -> io::Result<Option<(Frame<Bytes>, (tokio::fs::File, TempPath))>> {
    let mut buf = vec![0; READ_CHUNK];
    let n = file.read(&mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    buf.truncate(n);
    Ok(Some((Frame::data(Bytes::from(buf)), (file, path))))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::io::Read;

    use futures::StreamExt;
    use http_body_util::{Full, StreamBody};

    use super::*;

    const IDLE: Duration = Duration::from_secs(30);

    #[tokio::test(start_paused = true)]
    async fn a_stalled_body_times_out_and_keeps_the_head_read_so_far() {
        let dir = tempfile::tempdir().unwrap();
        let first = futures::stream::iter([Ok::<_, Infallible>(Frame::data(Bytes::from_static(
            b"head",
        )))]);
        let body = StreamBody::new(first.chain(futures::stream::pending()));

        let failure = spool(body, dir.path(), 1024, IDLE).await.unwrap_err();

        assert!(matches!(failure.error, SpoolError::TimedOut));
        assert_eq!(failure.head, b"head");
        assert_eq!(failure.read, 4);
    }

    #[tokio::test]
    async fn an_oversized_body_keeps_its_head_and_the_bytes_read() {
        let dir = tempfile::tempdir().unwrap();

        let failure = spool(
            Full::new(Bytes::from_static(b"0123456789")),
            dir.path(),
            4,
            IDLE,
        )
        .await
        .unwrap_err();

        assert_eq!(failure.head, b"0123456789");
        assert_eq!(failure.read, 10);
    }

    fn files_in(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    #[tokio::test]
    async fn small_body_is_spooled_and_streamed_back() {
        let dir = tempfile::tempdir().unwrap();
        let spool_dir = dir.path().join("spool");
        let body = Full::new(Bytes::from_static(b"hello world"));

        let spooled = spool(body, &spool_dir, 1024, IDLE).await.unwrap();

        assert_eq!(spooled.len(), 11);
        assert_eq!(spooled.head(), b"hello world");
        assert!(!spooled.is_probe());
        let mut rest = String::new();
        spooled
            .reader_at(6)
            .unwrap()
            .read_to_string(&mut rest)
            .unwrap();
        assert_eq!(rest, "world");
        let collected = spooled.into_body().collect().await.unwrap();
        assert_eq!(collected.to_bytes(), Bytes::from_static(b"hello world"));
        assert_eq!(files_in(&spool_dir), 0);
    }

    #[tokio::test]
    async fn chunked_multi_frame_body_is_concatenated() {
        let dir = tempfile::tempdir().unwrap();
        let chunks =
            (0..100u8).map(|i| Ok::<_, Infallible>(Frame::data(Bytes::from(vec![i; 1000]))));
        let body = StreamBody::new(futures::stream::iter(chunks));

        let spooled = spool(body, dir.path(), 1_000_000, IDLE).await.unwrap();

        assert_eq!(spooled.len(), 100_000);
        assert_eq!(spooled.head().len(), HEAD_LIMIT);
        let mut all = Vec::new();
        spooled.reader_at(0).unwrap().read_to_end(&mut all).unwrap();
        let expected: Vec<u8> = (0..100u8).flat_map(|i| vec![i; 1000]).collect();
        assert_eq!(all, expected);
    }

    #[tokio::test]
    async fn exceeding_the_limit_stops_reading_and_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let polled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = polled.clone();
        let chunks = futures::stream::iter(0..1000).map(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<_, Infallible>(Frame::data(Bytes::from(vec![0u8; 100])))
        });

        let err = spool(StreamBody::new(chunks), dir.path(), 250, IDLE)
            .await
            .unwrap_err();

        assert!(matches!(err.error, SpoolError::TooLarge { max_bytes: 250 }));
        assert_eq!(polled.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(files_in(dir.path()), 0);
    }

    #[tokio::test]
    async fn flush_only_body_is_a_probe() {
        let dir = tempfile::tempdir().unwrap();

        let probe = spool(
            Full::new(Bytes::from_static(b"0000")),
            dir.path(),
            1024,
            IDLE,
        )
        .await
        .unwrap();
        let push = spool(
            Full::new(Bytes::from_static(b"00000000")),
            dir.path(),
            1024,
            IDLE,
        )
        .await
        .unwrap();

        assert!(probe.is_probe());
        assert!(!push.is_probe());
    }

    #[tokio::test]
    async fn dropping_the_spool_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let spooled = spool(Full::new(Bytes::from_static(b"x")), dir.path(), 1024, IDLE)
            .await
            .unwrap();
        assert_eq!(files_in(dir.path()), 1);

        drop(spooled);

        assert_eq!(files_in(dir.path()), 0);
    }
}
