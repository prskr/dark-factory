//! What plugins can and cannot do: capability detection, ambient authority, resource limits.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use df_core::{AuthScheme, ForgeClient, IntegrationError, PluginConfig, ProjectId};
use df_plugins::{LoadError, PluginEngine, PluginHost};
use tracing_subscriber::fmt::MakeWriter;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;

fn probe_client(timeout: Duration, memory: usize) -> anyhow::Result<common::Client> {
    common::client(
        &common::probe(),
        "https://git.example.org/o/r.git",
        timeout,
        memory,
    )
}

#[test]
fn plugin_importing_filesystem_is_refused() {
    let component = r#"(component (import "wasi:filesystem/preopens@0.2.12" (instance)))"#;
    let err = PluginHost::from_bytes(
        &PluginEngine::new().expect("engine"),
        "probe",
        component.as_bytes(),
        Duration::from_secs(1),
        MIB,
    )
    .err()
    .expect("must be refused");
    assert!(
        matches!(&err, LoadError::ForbiddenImport(name) if name == "wasi:filesystem/preopens@0.2.12"),
        "{err}"
    );
}

#[test]
fn plugin_importing_an_unlisted_interface_of_an_allowed_namespace_is_refused() {
    for import in [
        "wasi:random/insecure@0.2.12",
        "wasi:http/incoming-handler@0.2.12",
    ] {
        let component = format!(r#"(component (import "{import}" (instance)))"#);
        let err = PluginHost::from_bytes(
            &PluginEngine::new().expect("engine"),
            "probe",
            component.as_bytes(),
            Duration::from_secs(1),
            MIB,
        )
        .err()
        .expect("must be refused");
        assert!(
            matches!(&err, LoadError::ForbiddenImport(name) if name == import),
            "{err}"
        );
    }
}

#[test]
fn component_without_capabilities_is_refused() {
    let err = PluginHost::from_bytes(
        &PluginEngine::new().expect("engine"),
        "probe",
        b"(component)",
        Duration::from_secs(1),
        MIB,
    )
    .err()
    .expect("must be refused");
    assert!(matches!(err, LoadError::NoCapability), "{err}");
}

#[tokio::test]
async fn plugin_sees_an_empty_environment() -> anyhow::Result<()> {
    let issue = probe_client(Duration::from_secs(30), 64 * MIB)?
        .get_issue(&ProjectId::new(), 1)
        .await?;
    assert_eq!(issue.body, "env=0");
    Ok(())
}

#[tokio::test]
async fn runaway_plugin_is_stopped_and_the_next_call_works() -> anyhow::Result<()> {
    let client = probe_client(Duration::from_millis(300), 64 * MIB)?;
    let project = ProjectId::new();
    let err = client.get_issue(&project, 2).await.unwrap_err();
    assert!(matches!(err, IntegrationError::Internal(_)), "{err}");
    assert_eq!(client.get_issue(&project, 1).await?.body, "env=0");
    Ok(())
}

#[tokio::test]
async fn plugin_exceeding_the_memory_limit_fails() -> anyhow::Result<()> {
    let client = probe_client(Duration::from_secs(30), 16 * MIB)?;
    let err = client.get_issue(&ProjectId::new(), 3).await.unwrap_err();
    assert!(matches!(err, IntegrationError::Internal(_)), "{err}");
    Ok(())
}

#[tokio::test]
async fn runaway_call_in_one_plugin_times_out_without_affecting_another_on_a_shared_engine()
-> anyhow::Result<()> {
    let engine = PluginEngine::new()?;
    let url = "https://git.example.org/o/r.git";
    let first = common::client_on(
        &engine,
        &common::probe(),
        url,
        common::TOKEN,
        Duration::from_secs(30),
        64 * MIB,
    )?;
    let second = common::client_on(
        &engine,
        &common::probe(),
        url,
        common::TOKEN,
        Duration::from_millis(300),
        64 * MIB,
    )?;
    let project = ProjectId::new();
    assert_eq!(first.get_issue(&project, 1).await?.body, "env=0");
    assert_eq!(second.get_issue(&project, 1).await?.body, "env=0");
    // The shared clock stops the runaway call of one plugin; the other keeps working.
    let err = second.get_issue(&project, 2).await.unwrap_err();
    assert!(matches!(err, IntegrationError::Internal(_)), "{err}");
    assert_eq!(first.get_issue(&project, 1).await?.body, "env=0");
    Ok(())
}

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

#[tokio::test]
async fn plugin_stderr_is_forwarded_to_the_log() -> anyhow::Result<()> {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_ansi(false)
        .finish();
    // Global, because thread-local subscribers race with tracing's callsite interest cache when
    // tests run in parallel. This is the only test of the binary that reads the log.
    tracing::subscriber::set_global_default(subscriber)?;
    probe_client(Duration::from_secs(30), 64 * MIB)?
        .get_issue(&ProjectId::new(), 4)
        .await?;
    let log = String::from_utf8(captured.0.lock().expect("lock").clone())?;
    assert!(log.contains("hello from stderr"), "{log}");
    assert!(log.contains("plugin=forgejo"), "{log}");
    Ok(())
}

fn http_probe_client(server: &MockServer) -> anyhow::Result<common::Client> {
    common::client(
        &common::probe(),
        &format!("{}/o/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )
}

#[tokio::test]
async fn host_replaces_an_authorization_header_set_by_the_plugin() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let issue = http_probe_client(&server)?
        .get_issue(&ProjectId::new(), 5)
        .await?;
    assert_eq!(issue.body, "200");
    let requests = server.received_requests().await.unwrap_or_default();
    let authorization: Vec<_> = requests[0]
        .headers
        .get_all("authorization")
        .iter()
        .collect();
    assert_eq!(authorization.len(), 1);
    assert_eq!(
        authorization[0].as_bytes(),
        format!("token {}", common::TOKEN).as_bytes()
    );
    Ok(())
}

#[tokio::test]
async fn bearer_scheme_of_the_plugin_config_sends_a_bearer_authorization_header()
-> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let mut config = PluginConfig::new("/forgejo.wasm");
    config.auth_scheme = AuthScheme::Bearer.into();
    let client = common::router_with_config(
        common::plugin_client(
            &PluginEngine::new()?,
            &common::probe(),
            common::TOKEN,
            Duration::from_secs(30),
            64 * MIB,
        )?,
        config,
        &format!("{}/o/r.git", server.uri()),
    )?;
    let issue = client.get_issue(&ProjectId::new(), 5).await?;
    assert_eq!(issue.body, "200");
    let requests = server.received_requests().await.unwrap_or_default();
    let authorization: Vec<_> = requests[0]
        .headers
        .get_all("authorization")
        .iter()
        .collect();
    assert_eq!(authorization.len(), 1);
    assert_eq!(
        authorization[0].as_bytes(),
        format!("Bearer {}", common::TOKEN).as_bytes()
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn request_to_another_host_fails_as_transport_error() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    // The probe requests `http://<repo owner>/`: here the other server.
    let owner = other.address().to_string();
    let err = common::client(
        &common::probe(),
        &format!("{}/{owner}/r.git", server.uri()),
        Duration::from_secs(30),
        64 * MIB,
    )?
    .get_issue(&ProjectId::new(), 6)
    .await
    .unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Transport(m) if m.contains("HttpRequestDenied")),
        "{err}"
    );
    assert!(
        other
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty()
    );
    Ok(())
}
