//! Captures log output in tests.

use std::sync::{Arc, Mutex};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

use crate::logging::{LogFormat, subscriber};

/// Log output captured by [`capture`] or [`buffer`].
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("capture lock")).into_owned()
    }
}

/// A writer factory appending to a fresh [`Captured`] buffer.
pub fn buffer() -> (
    Captured,
    impl for<'a> MakeWriter<'a> + Send + Sync + 'static,
) {
    let captured = Captured::default();
    let buf = Arc::clone(&captured.0);
    (captured, move || Sink(Arc::clone(&buf)))
}

/// Installs a thread-local subscriber (`filter` in `RUST_LOG` syntax) writing JSON lines to the
/// returned buffer until the guard is dropped.
pub fn capture(filter: &str) -> (Captured, tracing::subscriber::DefaultGuard) {
    static SILENT_GLOBAL: std::sync::Once = std::sync::Once::new();

    let (captured, writer) = buffer();
    let sub = subscriber(LogFormat::Json, EnvFilter::new(filter), writer, None);
    // With a single registered dispatcher, tracing computes a callsite's interest from the
    // registering thread's default only. A parallel test without a subscriber would then
    // disable the callsite for every capture. A second (silent, global) dispatcher makes
    // tracing consult all of them.
    SILENT_GLOBAL.call_once(|| {
        let silent = tracing_subscriber::fmt()
            .with_max_level(tracing::level_filters::LevelFilter::OFF)
            .with_writer(std::io::sink)
            .finish();
        // Ignored on purpose: a global subscriber may already exist, which serves as well.
        tracing::subscriber::set_global_default(silent).ok();
    });
    (captured, tracing::subscriber::set_default(sub))
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
