//! `PluginHost::load` reports what the component costs. Own test binary: it installs the global
//! tracing subscriber.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use df_plugins::{PluginEngine, PluginHost};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn load_logs_name_path_size_and_compile_time() -> anyhow::Result<()> {
    let captured = Captured::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(captured.clone())
            .with_ansi(false)
            .finish(),
    )?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("forgejo.wasm");
    let bytes = common::forgejo();
    std::fs::write(&path, &bytes)?;

    PluginHost::load(
        &PluginEngine::new()?,
        "forgejo",
        &path,
        Duration::from_secs(30),
        64 << 20,
    )?;

    let log = String::from_utf8(captured.0.lock().expect("lock").clone())?;
    assert!(log.contains(r#"plugin="forgejo""#), "{log}");
    assert!(log.contains(&format!("path={}", path.display())), "{log}");
    assert!(log.contains(&format!("bytes={}", bytes.len())), "{log}");
    assert!(log.contains("compile_ms="), "{log}");
    Ok(())
}
