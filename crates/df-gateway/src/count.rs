//! Byte counters that survive the cancellation of the code that moves the bytes.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use bytes::Bytes;
use hyper::body::{Body, Frame, SizeHint};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Bytes of one request, from the sandbox's point of view.
#[derive(Debug, Default)]
pub struct Traffic {
    /// Sandbox to destination.
    pub sent: AtomicU64,
    /// Destination to sandbox.
    pub received: AtomicU64,
}

impl Traffic {
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }
}

/// A connection to the destination that counts what is written to it (`sent`) and read from it
/// (`received`).
pub struct Counted<T> {
    inner: T,
    traffic: Arc<Traffic>,
}

impl<T> Counted<T> {
    pub fn new(inner: T, traffic: Arc<Traffic>) -> Self {
        Self { inner, traffic }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Counted<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if poll.is_ready() {
            let read = (buf.filled().len() - before) as u64;
            self.traffic.received.fetch_add(read, Ordering::Relaxed);
        }
        poll
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Counted<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, data);
        if let Poll::Ready(Ok(written)) = &poll {
            self.traffic
                .sent
                .fetch_add(*written as u64, Ordering::Relaxed);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Which counter of [`Traffic`] a [`Metered`] body adds to.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Sent,
    Received,
}

type OnEnd = Box<dyn FnOnce(&Traffic) + Send + Sync>;

/// An HTTP body that counts the data passing through it. `on_end` runs when the body is dropped,
/// whether it was read to its end or abandoned.
pub struct Metered<B> {
    inner: B,
    traffic: Arc<Traffic>,
    direction: Direction,
    on_end: Option<OnEnd>,
}

impl<B> Metered<B> {
    pub fn new(inner: B, traffic: Arc<Traffic>, direction: Direction) -> Self {
        Self {
            inner,
            traffic,
            direction,
            on_end: None,
        }
    }

    pub fn on_end(mut self, on_end: impl FnOnce(&Traffic) + Send + Sync + 'static) -> Self {
        self.on_end = Some(Box::new(on_end));
        self
    }
}

impl<B> Drop for Metered<B> {
    fn drop(&mut self) {
        if let Some(on_end) = self.on_end.take() {
            on_end(&self.traffic);
        }
    }
}

impl<B: Body<Data = Bytes> + Unpin> Body for Metered<B> {
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let poll = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &poll
            && let Some(data) = frame.data_ref()
        {
            let counter = match self.direction {
                Direction::Sent => &self.traffic.sent,
                Direction::Received => &self.traffic.received,
            };
            counter.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        poll
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::{BodyExt, Full};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn streams_count_both_directions() {
        let (near, mut far) = tokio::io::duplex(64);
        let traffic = Arc::new(Traffic::default());
        let mut counted = Counted::new(near, traffic.clone());
        counted.write_all(b"hello").await.unwrap();
        far.write_all(b"abc").await.unwrap();
        let mut buf = [0; 3];
        counted.read_exact(&mut buf).await.unwrap();
        assert_eq!((traffic.sent(), traffic.received()), (5, 3));
    }

    #[tokio::test]
    async fn bodies_count_their_data_and_report_when_dropped() {
        let traffic = Arc::new(Traffic::default());
        let seen = Arc::new(AtomicU64::new(0));
        let body = Metered::new(
            Full::new(Bytes::from_static(b"12345")),
            traffic.clone(),
            Direction::Received,
        )
        .on_end({
            let seen = seen.clone();
            move |traffic| seen.store(traffic.received(), Ordering::Relaxed)
        });
        let collected = body.collect().await.unwrap().to_bytes();
        assert_eq!(collected.len(), 5);
        // `collect` consumed and dropped the body.
        assert_eq!(seen.load(Ordering::Relaxed), 5);
        assert_eq!((traffic.sent(), traffic.received()), (0, 5));
    }
}
