//! Dark Factory control plane, database migration, and egress gateway binary.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use df_core::InstanceConfig;
use df_core::config::DEFAULT_CONFIG_PATH;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

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

    info!(
        instance = %cfg.instance_name,
        http_port = cfg.server.http_port,
        agent_port = cfg.server.agent_port,
        internal_grpc_port = cfg.server.internal_grpc_port,
        "starting control plane listeners"
    );

    let mut listeners = JoinSet::new();
    listeners.spawn(serve_http_health(http_listener, cfg.server.http_port));
    listeners.spawn(accept_discard_loop(agent_listener, cfg.server.agent_port));
    listeners.spawn(accept_discard_loop(
        internal_listener,
        cfg.server.internal_grpc_port,
    ));

    supervise_listeners("control plane", listeners, wait_for_shutdown_signal()).await
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

    supervise_listeners("gateway", listeners, wait_for_shutdown_signal()).await
}

async fn supervise_listeners<F>(
    component: &str,
    mut listeners: JoinSet<()>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()>,
{
    tokio::select! {
        _ = shutdown => {
            info!("received shutdown signal; terminating {component} listeners");
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
        let res = supervise_listeners("test", set, std::future::pending()).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("exited unexpectedly"));
    }

    #[tokio::test]
    async fn supervise_listeners_terminates_cleanly_on_shutdown_signal() {
        let mut set = JoinSet::new();
        set.spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let res = supervise_listeners("test", set, async {}).await;
        assert!(res.is_ok());
    }
}
