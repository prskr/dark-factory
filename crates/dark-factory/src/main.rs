//! Dark Factory control plane, database migration, and egress gateway binary.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use df_agent_channel::{ChannelConfig, ChannelContext, Scheduler};
use df_core::InstanceConfig;
use df_core::config::DEFAULT_CONFIG_PATH;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod unconfigured;

/// Time the control plane's listeners get to finish their work after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(
    name = "dark-factory",
    version,
    about = "Dark Factory control plane and gateway"
)]
pub struct Cli {
    /// Path to the instance configuration YAML file.
    #[arg(
        long,
        short = 'c',
        global = true,
        env = "DARK_FACTORY_CONFIG",
        default_value = DEFAULT_CONFIG_PATH
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Run the Dark Factory control plane server.
    Serve,
    /// Apply embedded PostgreSQL migrations, optional application role management, and grants.
    Migrate,
    /// Run the Dark Factory sandbox egress gateway.
    Gateway,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => match run_serve_command(&cli.config).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!(error = %err, "control plane server failed");
                ExitCode::from(1)
            }
        },
        Command::Migrate => match run_migrate_command(&cli.config).await {
            Ok(()) => {
                info!("database migration completed successfully");
                ExitCode::SUCCESS
            }
            Err(err) => {
                error!(error = %err, "database migration failed");
                ExitCode::from(1)
            }
        },
        Command::Gateway => match run_gateway_command(&cli.config).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!(error = %err, "egress gateway failed");
                ExitCode::from(1)
            }
        },
    }
}

async fn bind_listener(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await
}

async fn run_migrate_command(config_path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = InstanceConfig::from_file(config_path)?;
    df_db::run_migrate(&cfg.database).await?;
    Ok(())
}

async fn run_serve_command(config_path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = InstanceConfig::from_file(config_path)?;
    let http_listener = bind_listener(cfg.server.http_port).await?;
    let agent_listener = bind_listener(cfg.server.agent_port).await?;
    let internal_listener = bind_listener(cfg.server.internal_grpc_port).await?;
    serve(
        cfg,
        http_listener,
        agent_listener,
        internal_listener,
        wait_for_shutdown_signal(),
    )
    .await
}

/// Runs the control plane on the given listeners until `shutdown` resolves: UI/health port, the
/// mTLS agent port with its run scheduler, and the internal gRPC port.
async fn serve<F>(
    cfg: InstanceConfig,
    http_listener: TcpListener,
    agent_listener: TcpListener,
    internal_listener: TcpListener,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()>,
{
    let tls = cfg
        .agent
        .tls
        .as_ref()
        .context("`agent.tls` (certFile, keyFile, clientCaFile) must be configured")?;
    let tls = df_agent_channel::TlsReloader::load(tls).await?;
    let pool = df_db::connect_pool(&cfg.database).await?;
    let ctx = ChannelContext {
        notifier: Arc::new(df_db::Notifier::start(&pool).await?),
        pool,
        provider: Arc::new(unconfigured::UnconfiguredSandboxProvider),
        forge: Arc::new(unconfigured::UnconfiguredForgeClient),
        secrets: Arc::new(unconfigured::UnconfiguredSecretStore),
        config: Arc::new(ChannelConfig::from(&cfg)),
    };

    let http_port = http_listener.local_addr()?.port();
    let agent_port = agent_listener.local_addr()?.port();
    let internal_port = internal_listener.local_addr()?.port();
    info!(
        instance = %cfg.instance_name,
        http_port,
        agent_port,
        internal_grpc_port = internal_port,
        "starting control plane listeners"
    );

    // The listeners stop on request (the agent port and the scheduler finish their work first)
    // instead of being aborted mid-way.
    let (stop_tx, stop_rx) = watch::channel(false);
    let shutdown = async move {
        shutdown.await;
        stop_tx.send_replace(true);
    };

    let mut listeners = JoinSet::new();
    listeners.spawn(until_stopped(
        stop_rx.clone(),
        serve_http_health(http_listener, http_port),
    ));
    let agent_ctx = ctx.clone();
    let agent_stop = stop_rx.clone();
    listeners.spawn(async move {
        if let Err(err) =
            df_agent_channel::serve(agent_ctx, agent_listener, tls, stopped(agent_stop)).await
        {
            error!(error = %err, "agent port failed");
        }
    });
    let scheduler_stop = stop_rx.clone();
    listeners.spawn(async move { Scheduler::new(ctx).run(stopped(scheduler_stop)).await });
    listeners.spawn(until_stopped(
        stop_rx,
        accept_discard_loop(internal_listener, internal_port),
    ));

    supervise_listeners("control plane", listeners, shutdown, SHUTDOWN_GRACE).await
}

/// Resolves once `stop` was set.
async fn stopped(mut stop: watch::Receiver<bool>) {
    // An error means the sender is gone, which is a stop as well.
    let _ = stop.wait_for(|stop| *stop).await;
}

async fn until_stopped(stop: watch::Receiver<bool>, task: impl std::future::Future<Output = ()>) {
    tokio::select! {
        () = task => {}
        () = stopped(stop) => {}
    }
}

async fn run_gateway_command(config_path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = InstanceConfig::from_file(config_path)?;
    let health_listener = bind_listener(cfg.gateway.health_port).await?;
    let proxy_listener = bind_listener(cfg.gateway.proxy_port).await?;
    let internal_listener = bind_listener(cfg.gateway.internal_grpc_port).await?;

    info!(
        instance = %cfg.instance_name,
        health_port = cfg.gateway.health_port,
        proxy_port = cfg.gateway.proxy_port,
        internal_grpc_port = cfg.gateway.internal_grpc_port,
        "starting gateway listeners"
    );

    let mut listeners = JoinSet::new();
    listeners.spawn(serve_http_health(health_listener, cfg.gateway.health_port));
    listeners.spawn(serve_proxy_stub(proxy_listener, cfg.gateway.proxy_port));
    listeners.spawn(accept_discard_loop(
        internal_listener,
        cfg.gateway.internal_grpc_port,
    ));

    supervise_listeners(
        "gateway",
        listeners,
        wait_for_shutdown_signal(),
        Duration::ZERO,
    )
    .await
}

/// Runs until `shutdown` resolves or a listener exits. After a shutdown the listeners get `grace`
/// to finish (they must have been told to stop by then), the rest is aborted.
async fn supervise_listeners<F>(
    component: &str,
    mut listeners: JoinSet<()>,
    shutdown: F,
    grace: Duration,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()>,
{
    tokio::select! {
        _ = shutdown => {
            info!("received shutdown signal; terminating {component} listeners");
            if !grace.is_zero() {
                let finished = async { while listeners.join_next().await.is_some() {} };
                if tokio::time::timeout(grace, finished).await.is_err() {
                    error!("{component} listeners still running after the grace period");
                }
            }
            listeners.abort_all();
            Ok(())
        }
        Some(res) = listeners.join_next() => {
            listeners.abort_all();
            match res {
                Ok(()) => Err(anyhow::anyhow!("a {component} listener exited unexpectedly")),
                Err(err) => Err(anyhow::anyhow!("a {component} listener panicked or failed: {err}")),
            }
        }
    }
}

async fn serve_http_health(listener: TcpListener, port: u16) {
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                error!(error = %err, port = port, "accept failed on health listener; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = std::str::from_utf8(&buf[..n]).unwrap_or("");
            let response = if req.starts_with("GET /assets/app.css ") {
                let css = df_web::app_css().unwrap_or_default();
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/css; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    css.len(),
                    css
                )
            } else {
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_string()
            };
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

async fn serve_proxy_stub(listener: TcpListener, port: u16) {
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                error!(error = %err, port = port, "accept failed on proxy listener; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = std::str::from_utf8(&buf[..n]).unwrap_or("");
            let response = if req.starts_with("CONNECT ") {
                "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 29\r\nConnection: close\r\n\r\nEgress proxy not initialized\n"
            } else {
                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nContent-Length: 29\r\nConnection: close\r\n\r\nEgress proxy not initialized\n"
            };
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

async fn accept_discard_loop(listener: TcpListener, port: u16) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => drop(stream),
            Err(err) => {
                error!(error = %err, port = port, "accept failed on discard listener; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;

    #[test]
    fn parses_subcommands() {
        let cli = Cli::try_parse_from(["dark-factory", "-c", "/tmp/cfg.yaml", "serve"]).unwrap();
        assert_eq!(cli.command, Command::Serve);
        assert_eq!(cli.config, PathBuf::from("/tmp/cfg.yaml"));
    }

    #[tokio::test]
    async fn proxy_stub_returns_503_for_connect_tunnel() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_proxy_stub(listener, port));

        let mut client = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        client
            .write_all(b"CONNECT api.example.com:443 HTTP/1.1\r\nHost: api.example.com:443\r\n\r\n")
            .await
            .unwrap();

        let mut buf = [0_u8; 512];
        let n = client.read(&mut buf).await.unwrap();
        let resp = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(resp.contains("Egress proxy not initialized"));
    }

    #[tokio::test]
    async fn proxy_stub_returns_502_for_http_proxy_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_proxy_stub(listener, port));

        let mut client = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        client
            .write_all(
                b"GET http://api.example.com/health HTTP/1.1\r\nHost: api.example.com\r\n\r\n",
            )
            .await
            .unwrap();

        let mut buf = [0_u8; 512];
        let n = client.read(&mut buf).await.unwrap();
        let resp = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 502 Bad Gateway\r\n"));
        assert!(resp.contains("Egress proxy not initialized"));
    }

    #[tokio::test]
    async fn http_health_serves_healthz_and_assets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve_http_health(listener, port));

        // Test /healthz
        let mut client = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        client
            .write_all(b"GET /healthz HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0_u8; 512];
        let n = client.read(&mut buf).await.unwrap();
        let resp = std::str::from_utf8(&buf[..n]).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.ends_with("\r\n\r\nok"));

        // Test /assets/app.css
        let mut client2 = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        client2
            .write_all(b"GET /assets/app.css HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut buf2 = [0_u8; 512];
        let n2 = client2.read(&mut buf2).await.unwrap();
        let resp2 = std::str::from_utf8(&buf2[..n2]).unwrap();
        assert!(resp2.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp2.contains("Content-Type: text/css"));
    }

    #[tokio::test]
    async fn supervise_listeners_detects_exited_listener_task() {
        let mut set = JoinSet::new();
        // Spawn a task that exits immediately
        set.spawn(async {});
        let res = supervise_listeners("test", set, std::future::pending(), Duration::ZERO).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("exited unexpectedly"));
    }

    #[tokio::test]
    async fn supervise_listeners_terminates_cleanly_on_shutdown_signal() {
        let mut set = JoinSet::new();
        set.spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let res = supervise_listeners("test", set, async {}, Duration::ZERO).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn supervise_listeners_lets_listeners_finish_within_the_grace_period() {
        let (stop_tx, stop_rx) = watch::channel(false);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let mut set = JoinSet::new();
        set.spawn(async move {
            stopped(stop_rx).await;
            // Work that outlasts the shutdown signal.
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = done_tx.send(());
        });

        let shutdown = async move {
            stop_tx.send_replace(true);
        };
        supervise_listeners("test", set, shutdown, Duration::from_secs(10))
            .await
            .unwrap();

        assert!(done_rx.await.is_ok(), "the listener was aborted");
    }

    fn serve_config(
        pool: &sqlx::PgPool,
        dir: &std::path::Path,
        tls: Option<&str>,
    ) -> InstanceConfig {
        let options = pool.connect_options();
        std::fs::write(dir.join("db-password"), "postgres\n").unwrap();
        let tls = tls.unwrap_or_default();
        let yaml = format!(
            "database:\n  host: {}\n  port: {}\n  database: {}\n  user: {}\n  passwordFile: {}/db-password\n  schema: public\n{tls}",
            options.get_host(),
            options.get_port(),
            options.get_database().unwrap(),
            options.get_username(),
            dir.display(),
        );
        InstanceConfig::from_yaml_str(&yaml).unwrap()
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn serve_runs_the_agent_port_next_to_the_ui_port(
        pool: sqlx::PgPool,
    ) -> anyhow::Result<()> {
        use df_agent_channel::testing::{POD_SAN, RuntimeClient, TestPki, TestServer};

        let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
        let dir = tempfile::tempdir()?;
        let files = pki.write_agent_tls(dir.path())?;
        let tls = format!(
            "agent:\n  tls:\n    certFile: {}\n    keyFile: {}\n    clientCaFile: {}\n",
            files.cert_file.display(),
            files.key_file.display(),
            files.client_ca_file.display(),
        );
        let cfg = serve_config(&pool, dir.path(), Some(&tls));

        let http = TcpListener::bind("127.0.0.1:0").await?;
        let agent = TcpListener::bind("127.0.0.1:0").await?;
        let internal = TcpListener::bind("127.0.0.1:0").await?;
        let (http_addr, agent_addr) = (http.local_addr()?, agent.local_addr()?);
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let control_plane = tokio::spawn(serve(cfg, http, agent, internal, async {
            let _ = stopped.await;
        }));

        // UI port answers.
        let mut ui = TcpStream::connect(http_addr).await?;
        ui.write_all(b"GET /healthz HTTP/1.1\r\n\r\n").await?;
        let mut buf = [0_u8; 64];
        let n = ui.read(&mut buf).await?;
        assert!(std::str::from_utf8(&buf[..n])?.starts_with("HTTP/1.1 200 OK"));

        // Agent port does mTLS and authenticates against the provider: the placeholder provider
        // knows no sandbox, so a valid client certificate is rejected as unauthenticated.
        let client = pki.pod_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
        let channel = TestServer::channel_to(agent_addr, &pki, Some(&client)).await?;
        let status = RuntimeClient::open(channel)
            .await
            .err()
            .expect("no sandbox is known");
        assert_eq!(status.code(), tonic::Code::Unauthenticated);

        stop.send(()).ok();
        control_plane.await??;
        Ok(())
    }

    #[sqlx::test(migrator = "df_db::MIGRATOR")]
    async fn serve_requires_agent_tls_configuration(pool: sqlx::PgPool) -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let cfg = serve_config(&pool, dir.path(), None);
        let listeners = [
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
        ];
        let [http, agent, internal] = listeners;
        let err = serve(cfg, http, agent, internal, std::future::pending())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("agent.tls"), "{err}");
        Ok(())
    }
}
