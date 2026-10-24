//! Dark Factory control plane, database migration, and egress gateway binary.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use df_agent_channel::{ChannelConfig, ChannelContext, Scheduler};
use df_core::InstanceConfig;
use df_core::config::DEFAULT_CONFIG_PATH;
use df_db::{Notification, PgPool};
use df_k8s::{
    ClusterServiceRef, ClusterTargetResolver, ClusterTargetsProvider, K8S_API_TIMEOUT,
    K8sSandboxProvider, K8sSecretStore, ProvisioningConfig, ResolvedClusterTarget,
    run_leader_election, run_policy_reconcile_loop, verify_startup, watch_cluster_target_changes,
};
use df_web::egress::LookupError;
use kube::Client;
use kube::api::Api;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod forge;
mod unconfigured;

/// Time the control plane's listeners get to finish their work after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);
/// Name of the forge integration plugin (`plugins.forgejo`); the only one supported for now.
const FORGE_PLUGIN: &str = "forgejo";

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
    Serve {
        /// Local development only: run without a Kubernetes cluster. The UI works; every sandbox
        /// operation (starting a run) fails.
        #[arg(long)]
        no_cluster: bool,
    },
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
    // Ignored on purpose: an already installed provider is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Serve { no_cluster } => match run_serve_command(&cli.config, no_cluster).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!(error = format!("{err:#}"), "control plane server failed");
                ExitCode::from(1)
            }
        },
        Command::Migrate => match run_migrate_command(&cli.config).await {
            Ok(()) => {
                info!("database migration completed successfully");
                ExitCode::SUCCESS
            }
            Err(err) => {
                error!(error = format!("{err:#}"), "database migration failed");
                ExitCode::from(1)
            }
        },
        Command::Gateway => match run_gateway_command(&cli.config).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!(error = format!("{err:#}"), "egress gateway failed");
                ExitCode::from(1)
            }
        },
    }
}

/// Cluster targets from the egress allowlist in the database, resolved against the cluster.
struct DbClusterTargetsProvider {
    pool: PgPool,
    resolver: ClusterTargetResolver,
}

#[async_trait::async_trait]
impl ClusterTargetsProvider for DbClusterTargetsProvider {
    /// Fails only if the allowlist cannot be read (provisioning fails; reconciliation keeps the
    /// run's current policy). A rule whose Service cannot be resolved is skipped, or kept at its
    /// last good resolution, with a warning naming the rule (see [`ClusterTargetResolver`]).
    async fn get_targets(
        &self,
        project_key: Option<&str>,
    ) -> Result<Vec<ResolvedClusterTarget>, df_core::seams::ProviderError> {
        let rules: Vec<ClusterServiceRef> =
            df_db::egress::list_cluster_services(&self.pool, project_key)
                .await
                .map_err(|e| {
                    df_core::seams::ProviderError::Backend(format!(
                        "db error querying egress allowlist: {e}"
                    ))
                })?
                .into_iter()
                .map(|rule| ClusterServiceRef {
                    namespace: rule.namespace,
                    service: rule.service,
                    ports: rule.ports,
                    allow_dns: rule.allow_dns,
                })
                .collect();
        Ok(self.resolver.resolve(project_key, &rules).await)
    }
}

type Task = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The sandbox provider, secret store and background tasks the control plane runs with.
struct SandboxWiring {
    provider: Arc<dyn df_core::SandboxProvider>,
    secrets: Arc<dyn df_core::SecretStore>,
    /// Tasks that end themselves when `stop` is set (see [`SandboxBackend::start`]).
    tasks: Vec<Task>,
    /// Checks `cluster-service` allowlist entries against the cluster; `None` without a cluster.
    services: Option<Arc<dyn df_web::egress::ServiceLookup>>,
}

/// Looks up Services in the cluster the control plane runs in, in the namespaces it may read
/// (`clusterTargets`).
struct K8sServices {
    client: Client,
    targets: df_core::config::ClusterTargetsConfig,
}

impl K8sServices {
    fn check_namespace(&self, namespace: &str) -> Result<(), LookupError> {
        if self.targets.any_namespace || self.targets.namespaces.iter().any(|n| n == namespace) {
            Ok(())
        } else {
            Err(LookupError::NamespaceNotAllowed(namespace.to_string()))
        }
    }
}

#[async_trait::async_trait]
impl df_web::egress::ServiceLookup for K8sServices {
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError> {
        self.check_namespace(namespace)?;
        let api: Api<k8s_openapi::api::core::v1::Service> =
            Api::namespaced(self.client.clone(), namespace);
        tokio::time::timeout(K8S_API_TIMEOUT, api.get_opt(service))
            .await
            .map_err(|_| LookupError::Timeout)?
            .map(|found| found.is_some())
            .map_err(|e| LookupError::Api(e.to_string()))
    }
}

/// The live egress log: the relay to the gateway pods.
struct RelayFlows(df_gateway::control::Relay);

impl df_web::egress::FlowSource for RelayFlows {
    fn subscribe(&self) -> futures::stream::BoxStream<'static, df_proto::gateway::v1::FlowEvent> {
        self.0.subscribe()
    }
}

/// Where sandboxes come from.
#[async_trait::async_trait]
trait SandboxBackend {
    /// Verifies the environment and builds the provider. The returned tasks must end once
    /// `stop` is set (or its sender is gone).
    async fn start(
        self,
        cfg: &InstanceConfig,
        pool: &PgPool,
        notifier: &df_db::Notifier,
        stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring>;
}

/// Backend without a cluster (`serve --no-cluster`, tests): every sandbox operation fails.
struct UnconfiguredBackend;

#[async_trait::async_trait]
impl SandboxBackend for UnconfiguredBackend {
    async fn start(
        self,
        _cfg: &InstanceConfig,
        _pool: &PgPool,
        _notifier: &df_db::Notifier,
        _stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring> {
        Ok(SandboxWiring {
            provider: Arc::new(unconfigured::sandbox_placeholders::UnconfiguredSandboxProvider),
            secrets: Arc::new(unconfigured::sandbox_placeholders::UnconfiguredSecretStore),
            tasks: Vec::new(),
            services: None,
        })
    }
}

/// Sandboxes as agent-sandbox resources in the connected Kubernetes cluster.
struct KubernetesBackend {
    client: Client,
}

#[async_trait::async_trait]
impl SandboxBackend for KubernetesBackend {
    async fn start(
        self,
        cfg: &InstanceConfig,
        pool: &PgPool,
        notifier: &df_db::Notifier,
        stop: watch::Receiver<bool>,
    ) -> anyhow::Result<SandboxWiring> {
        let kube_client = self.client;
        verify_startup(
            &kube_client,
            &cfg.sandbox.provisioning.namespace,
            &cfg.sandbox.provisioning.instance_namespace,
            &cfg.cluster_targets,
        )
        .await
        .context("startup check failed")?;

        let (cp_ip, gw_ip) =
            resolve_cluster_ips(&kube_client, &cfg.instance_name, &cfg.sandbox).await?;

        let tls_server_name = cfg
            .sandbox
            .endpoints
            .control_plane_tls_server_name
            .clone()
            .unwrap_or_else(|| format!("{}-control-plane", cfg.instance_name));

        let prov_config = ProvisioningConfig::from_sandbox_config(
            cfg.instance_name.clone(),
            &cfg.sandbox.provisioning,
            df_k8s::ControlPlaneEndpoint {
                ip: cp_ip,
                port: cfg.server.agent_port,
                tls_server_name,
            },
            df_k8s::GatewayEndpoint {
                ip: gw_ip,
                proxy_port: cfg.gateway.proxy_port,
            },
        );

        let secrets: Arc<dyn df_core::SecretStore> = Arc::new(K8sSecretStore::new(
            kube_client.clone(),
            cfg.sandbox.provisioning.instance_namespace.clone(),
        ));

        let targets_provider: Arc<dyn ClusterTargetsProvider> =
            Arc::new(DbClusterTargetsProvider {
                pool: pool.clone(),
                resolver: ClusterTargetResolver::new(kube_client.clone()),
            });

        let (provider, kube_watcher_task) = K8sSandboxProvider::start(
            kube_client.clone(),
            prov_config.clone(),
            targets_provider.clone(),
        );

        let (is_leader_tx, is_leader_rx) = watch::channel(false);
        let lease_name = format!("{}-leader", cfg.instance_name);
        let identity = format!("{}-{}", cfg.instance_name, uuid::Uuid::now_v7());
        let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);

        // Forward database policy notifications with lag handling
        let mut db_notify_sub = notifier.subscribe();
        let n_tx_db = notify_tx.clone();
        let forwarder_task = async move {
            loop {
                match db_notify_sub.recv().await {
                    Ok(Notification::EgressPolicy | Notification::Reconnected) => {
                        // Ignored on purpose: a full channel means a pass is already pending,
                        // a closed one means the loop stopped.
                        let _ = n_tx_db.try_send(());
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(
                            skipped_notifications = n,
                            "database notification subscriber lagged; triggering policy reconciliation"
                        );
                        // Ignored on purpose: a full channel means a pass is already pending,
                        // a closed one means the loop stopped.
                        let _ = n_tx_db.try_send(());
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        };

        // Service/EndpointSlice changes in the cluster-target namespaces trigger a reconcile.
        let target_watch_task = watch_cluster_target_changes(
            kube_client.clone(),
            cfg.cluster_targets.clone(),
            notify_tx,
        );

        let leader_client = kube_client.clone();
        let leader_ns = cfg.sandbox.provisioning.instance_namespace.clone();
        let leader_stop = stop.clone();
        let leader_task = async move {
            if let Err(err) = run_leader_election(
                leader_client,
                leader_ns,
                lease_name,
                identity,
                df_k8s::reconcile::DEFAULT_LEASE_DURATION_SECONDS,
                is_leader_tx,
                leader_stop,
            )
            .await
            {
                error!(error = %err, "leader election failed");
            }
        };

        let services: Arc<dyn df_web::egress::ServiceLookup> = Arc::new(K8sServices {
            client: kube_client.clone(),
            targets: cfg.cluster_targets.clone(),
        });

        let reconcile_task = run_policy_reconcile_loop(
            kube_client,
            prov_config,
            targets_provider,
            is_leader_rx,
            notify_rx,
            stop.clone(),
        );

        Ok(SandboxWiring {
            provider: provider as Arc<dyn df_core::SandboxProvider>,
            secrets,
            services: Some(services),
            tasks: vec![
                Box::pin(until_stopped(stop.clone(), kube_watcher_task)),
                Box::pin(until_stopped(stop.clone(), forwarder_task)),
                Box::pin(until_stopped(stop, target_watch_task)),
                Box::pin(leader_task),
                Box::pin(reconcile_task),
            ],
        })
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

async fn run_serve_command(config_path: &std::path::Path, no_cluster: bool) -> anyhow::Result<()> {
    let cfg = InstanceConfig::from_file(config_path)?;
    let http_listener = bind_listener(cfg.server.http_port).await?;
    let agent_listener = bind_listener(cfg.server.agent_port).await?;
    let internal_listener = bind_listener(cfg.server.internal_grpc_port).await?;
    if no_cluster {
        tracing::warn!("running without a Kubernetes cluster; sandbox operations fail");
        return serve(
            cfg,
            UnconfiguredBackend,
            http_listener,
            agent_listener,
            internal_listener,
            wait_for_shutdown_signal(),
        )
        .await;
    }
    let client = Client::try_default()
        .await
        .context("failed to initialize Kubernetes client")?;
    serve(
        cfg,
        KubernetesBackend { client },
        http_listener,
        agent_listener,
        internal_listener,
        wait_for_shutdown_signal(),
    )
    .await
}

/// Runs the control plane on the given listeners until `shutdown` resolves: UI/health port, the
/// mTLS agent port with its run scheduler, and the internal gRPC port.
async fn serve<B, F>(
    cfg: InstanceConfig,
    backend: B,
    http_listener: TcpListener,
    agent_listener: TcpListener,
    internal_listener: TcpListener,
    shutdown: F,
) -> anyhow::Result<()>
where
    B: SandboxBackend,
    F: Future<Output = ()>,
{
    let tls_config = cfg
        .agent
        .tls
        .as_ref()
        .context("`agent.tls` (certFile, keyFile, clientCaFile) must be configured")?;
    let tls = df_agent_channel::TlsReloader::load(tls_config).await?;
    let internal_tls = df_agent_channel::TlsReloader::load(tls_config).await?;
    // One forge plugin is supported for now and `serve` refuses to start without it. With several
    // plugins this becomes: fail only if no forge plugin could be loaded. Loaded before the
    // database connection, so a missing component fails fast without a database.
    let forgejo = cfg
        .plugins
        .get(FORGE_PLUGIN)
        .cloned()
        .unwrap_or_else(|| df_core::PluginConfig::new("/plugins/forgejo.wasm"));
    let forge_host = forge::load_forge_plugin(FORGE_PLUGIN, &forgejo)?;
    let pool = df_db::connect_pool(&cfg.database).await?;
    let notifier = Arc::new(df_db::Notifier::start(&pool).await?);

    let (stop_tx, stop_rx) = watch::channel(false);

    let SandboxWiring {
        provider,
        secrets,
        tasks: sandbox_tasks,
        services,
    } = backend
        .start(&cfg, &pool, &notifier, stop_rx.clone())
        .await?;

    let forge = forge::forge_client(forge_host, &forgejo, pool.clone(), secrets.clone());
    let flows = egress_log_relay(&cfg, tls_config);
    let web = web_router(
        &cfg,
        &pool,
        &notifier,
        Some(df_core::ForgeKind::from_plugin(FORGE_PLUGIN)),
        EgressDeps { flows, services },
    )?;
    let ctx_policy = (pool.clone(), notifier.clone());
    let ctx = ChannelContext {
        notifier,
        pool,
        provider,
        forge,
        secrets,
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

    let shutdown = async move {
        shutdown.await;
        stop_tx.send_replace(true);
    };

    let mut listeners = JoinSet::new();
    listeners.spawn(until_stopped(stop_rx.clone(), serve_ui(http_listener, web)));
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
    listeners.spawn(serve_policy_watch(
        cfg.gateway.identity_name(&cfg.instance_name),
        ctx_policy,
        df_agent_channel::mtls_incoming(internal_listener, internal_tls),
        stop_rx.clone(),
    ));
    for task in sandbox_tasks {
        listeners.spawn(task);
    }

    supervise_listeners("control plane", listeners, shutdown, SHUTDOWN_GRACE).await
}

async fn resolve_cluster_ips(
    client: &Client,
    instance_name: &str,
    sandbox: &df_core::SandboxSectionConfig,
) -> anyhow::Result<(IpAddr, IpAddr)> {
    let instance_namespace = sandbox.provisioning.instance_namespace.as_str();
    let (configured_cp_ip, configured_gw_ip) = (
        sandbox.endpoints.control_plane_ip,
        sandbox.endpoints.gateway_ip,
    );
    let (cp_service_name, gw_service_name) = (
        sandbox.endpoints.control_plane_service_name.as_deref(),
        sandbox.endpoints.gateway_service_name.as_deref(),
    );
    async fn resolve_service_ip(
        client: &Client,
        namespace: &str,
        names: &[String],
    ) -> anyhow::Result<IpAddr> {
        let svcs: Api<k8s_openapi::api::core::v1::Service> =
            Api::namespaced(client.clone(), namespace);
        let mut last_error = None;
        for name in names {
            let get_fut = tokio::time::timeout(K8S_API_TIMEOUT, svcs.get(name));
            match get_fut.await {
                Ok(Ok(svc)) => {
                    if let Some(ip) = svc
                        .spec
                        .as_ref()
                        .and_then(|s| s.cluster_ip.as_ref())
                        .and_then(|ip| ip.parse::<IpAddr>().ok())
                    {
                        return Ok(ip);
                    }
                }
                Ok(Err(err)) => {
                    last_error = Some(anyhow::anyhow!(err));
                }
                Err(_) => {
                    last_error = Some(anyhow::anyhow!("timeout resolving service `{name}`"));
                }
            }
        }
        let tried = names.join(", ");
        if let Some(err) = last_error {
            anyhow::bail!(
                "failed to resolve cluster IP for service in namespace `{namespace}` (tried: {tried}): {err:#}"
            );
        } else {
            anyhow::bail!(
                "service in namespace `{namespace}` has no valid cluster IP (tried: {tried})"
            );
        }
    }

    let mut cp_names = Vec::new();
    if let Some(name) = cp_service_name {
        cp_names.push(name.to_string());
    }
    cp_names.push(format!("{instance_name}-control-plane"));
    cp_names.push("dark-factory-control-plane".to_string());

    let cp_ip = match configured_cp_ip {
        Some(ip) => ip,
        None => resolve_service_ip(client, instance_namespace, &cp_names)
            .await
            .context("failed to resolve control plane service IP")?,
    };

    let mut gw_names = Vec::new();
    if let Some(name) = gw_service_name {
        gw_names.push(name.to_string());
    }
    gw_names.push(format!("{instance_name}-gateway"));
    gw_names.push(format!("{instance_name}-egress-gateway"));
    gw_names.push("dark-factory-gateway".to_string());

    let gw_ip = match configured_gw_ip {
        Some(ip) => ip,
        None => resolve_service_ip(client, instance_namespace, &gw_names)
            .await
            .context("failed to resolve egress gateway service IP")?,
    };

    Ok((cp_ip, gw_ip))
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
    let tls = cfg
        .agent
        .tls
        .clone()
        .context("`agent.tls` (certFile, keyFile, clientCaFile) must be configured")?;
    let health_listener = bind_listener(cfg.gateway.health_port).await?;
    let proxy_listener = bind_listener(cfg.gateway.proxy_port).await?;
    let internal_listener = bind_listener(cfg.gateway.internal_grpc_port).await?;
    let control_plane_name = cfg.gateway.control_plane_name(&cfg.instance_name);
    // Kubernetes sets HOSTNAME to the pod name.
    let pod = std::env::var("HOSTNAME").unwrap_or_else(|_| "gateway".to_string());

    info!(
        instance = %cfg.instance_name,
        %pod,
        health_port = cfg.gateway.health_port,
        proxy_port = cfg.gateway.proxy_port,
        internal_grpc_port = cfg.gateway.internal_grpc_port,
        "starting gateway listeners"
    );

    let settings = df_gateway::gateway::Settings {
        pod,
        control_plane: format!("{control_plane_name}:{}", cfg.server.internal_grpc_port),
        buffer_size: cfg.gateway.buffer_size,
        blocked_cidrs: cfg.gateway.blocked_cidrs.clone(),
        retry: df_gateway::watch::RETRY_INTERVAL,
        control_plane_name: Some(control_plane_name.clone()),
    };
    let outside = df_gateway::gateway::Outside {
        connector: Arc::new(df_gateway::link::MtlsConnector::new(
            tls.clone(),
            control_plane_name,
        )),
        resolver: Arc::new(df_gateway::resolve::SystemResolver::new()?),
        dialer: Arc::new(df_gateway::proxy::TcpDialer),
    };
    let tail = df_agent_channel::mtls_incoming(
        internal_listener,
        df_agent_channel::TlsReloader::load(&tls).await?,
    );

    let shutdown = tokio_util::sync::CancellationToken::new();
    let signal = shutdown.clone();
    let signals = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        info!("received shutdown signal; stopping the gateway");
        signal.cancel();
    });
    let result = df_gateway::gateway::run(
        settings,
        tokio::sync::watch::channel(None).0,
        outside,
        health_listener,
        proxy_listener,
        tail,
        shutdown,
    )
    .await;
    signals.abort();
    Ok(result?)
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

/// The router of the UI port: the full UI if `auth` is configured, otherwise only health checks
/// and assets.
fn web_router(
    cfg: &InstanceConfig,
    pool: &PgPool,
    notifier: &Arc<df_db::Notifier>,
    forge: Option<df_core::ForgeKind>,
    egress: EgressDeps,
) -> anyhow::Result<axum::Router> {
    let Some(auth) = &cfg.auth else {
        tracing::warn!("`auth` is not configured; the web UI is disabled");
        return Ok(df_web::base_router(pool.clone()));
    };
    let oidc = df_web::auth::OidcClient::new(auth).context("invalid `auth` configuration")?;
    Ok(df_web::router(df_web::WebState {
        pool: pool.clone(),
        notifier: notifier.clone(),
        oidc: Arc::new(oidc),
        forge,
        flows: egress.flows,
        services: egress.services,
    }))
}

/// What the egress pages need from the outside.
#[derive(Default)]
struct EgressDeps {
    flows: Option<Arc<dyn df_web::egress::FlowSource>>,
    services: Option<Arc<dyn df_web::egress::ServiceLookup>>,
}

/// The relay to the gateway pods; none without `gateway.headlessService`.
fn egress_log_relay(
    cfg: &InstanceConfig,
    tls: &df_core::config::AgentTlsConfig,
) -> Option<Arc<dyn df_web::egress::FlowSource>> {
    let service = cfg.gateway.headless_service.as_deref()?;
    let connector = df_gateway::link::MtlsConnector::new(
        tls.clone(),
        cfg.gateway.identity_name(&cfg.instance_name),
    );
    let directory = df_gateway::control::DnsDirectory::new(
        service,
        cfg.gateway.internal_grpc_port,
        Arc::new(connector),
    );
    let relay = df_gateway::control::Relay::new(
        Arc::new(directory),
        df_gateway::control::relay::DISCOVERY_INTERVAL,
    );
    Some(Arc::new(RelayFlows(relay)))
}

/// Serves `PolicyWatch` to the gateway pods on the internal port until `stop` is set.
async fn serve_policy_watch(
    gateway: String,
    (pool, notifier): (PgPool, Arc<df_db::Notifier>),
    incoming: df_agent_channel::MtlsIncoming,
    stop: watch::Receiver<bool>,
) {
    let token = tokio_util::sync::CancellationToken::new();
    let service = df_gateway::control::PolicyService::new(pool, notifier, token.clone());
    let cancel = token.clone();
    let canceller = async move {
        stopped(stop).await;
        cancel.cancel();
    };
    let (served, ()) = tokio::join!(
        df_gateway::control::serve_policy(service, incoming, Some(gateway), token),
        canceller
    );
    if let Err(err) = served {
        error!(error = %err, "policy watch server failed");
    }
}

async fn serve_ui(listener: TcpListener, router: axum::Router) {
    if let Err(err) = axum::serve(listener, router).await {
        error!(error = %err, "UI listener failed");
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
        // Both outcomes (signal, handler error) mean: shut down.
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn service_lookup_is_limited_to_the_cluster_target_namespaces() {
        // No request reaches the API for a namespace that is not allowed.
        let client =
            Client::try_from(kube::Config::new("http://127.0.0.1:1".parse().unwrap())).unwrap();
        let limited = K8sServices {
            client: client.clone(),
            targets: df_core::config::ClusterTargetsConfig {
                any_namespace: false,
                namespaces: vec!["nativelink".into()],
            },
        };
        assert!(limited.check_namespace("nativelink").is_ok());
        let denied =
            df_web::egress::ServiceLookup::exists(&limited, "kube-system", "kube-dns").await;
        assert!(matches!(denied, Err(LookupError::NamespaceNotAllowed(ns)) if ns == "kube-system"));
        let any = K8sServices {
            client,
            targets: df_core::config::ClusterTargetsConfig {
                any_namespace: true,
                namespaces: vec![],
            },
        };
        assert!(any.check_namespace("anything").is_ok());
    }

    #[test]
    fn parses_subcommands() {
        let cli = Cli::try_parse_from(["dark-factory", "-c", "/tmp/cfg.yaml", "serve"]).unwrap();
        assert_eq!(cli.command, Command::Serve { no_cluster: false });
        assert_eq!(cli.config, PathBuf::from("/tmp/cfg.yaml"));
        let cli = Cli::try_parse_from(["dark-factory", "serve", "--no-cluster"]).unwrap();
        assert_eq!(cli.command, Command::Serve { no_cluster: true });
    }

    async fn ui_response(router: axum::Router, request: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_ui(listener, router));
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let mut buf = vec![0_u8; 4096];
        let n = client.read(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf[..n]).to_lowercase()
    }

    #[tokio::test]
    async fn ui_port_requires_login_when_auth_is_configured() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("oidc-secret"), "s3cret\n")?;
        let mut cfg = serve_config(&pool, dir.path(), None);
        cfg.auth = Some(df_core::AuthConfig {
            issuer_url: "https://idp.example.org/realms/df".into(),
            client_id: "dark-factory".into(),
            client_secret_file: dir.path().join("oidc-secret"),
            public_url: "https://factory.example.org".into(),
            roles_claim: "roles".into(),
            extra_scopes: Vec::new(),
        });
        let notifier = Arc::new(df_db::Notifier::start(&pool).await?);
        let router = web_router(&cfg, &pool, &notifier, None, EgressDeps::default())?;

        let tasks = ui_response(router.clone(), "GET /tasks HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(tasks.starts_with("http/1.1 303"), "{tasks}");
        assert!(tasks.contains("location: /login?next=%2ftasks"), "{tasks}");
        let health = ui_response(router, "GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(health.starts_with("http/1.1 200"), "{health}");
        Ok(())
    }

    #[tokio::test]
    async fn ui_is_disabled_without_auth_but_health_stays_up() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let dir = tempfile::tempdir()?;
        let cfg = serve_config(&pool, dir.path(), None);
        let notifier = Arc::new(df_db::Notifier::start(&pool).await?);
        let router = web_router(&cfg, &pool, &notifier, None, EgressDeps::default())?;
        let tasks = ui_response(router.clone(), "GET /tasks HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(tasks.starts_with("http/1.1 404"), "{tasks}");
        let ready = ui_response(router, "GET /readyz HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(ready.starts_with("http/1.1 200"), "{ready}");
        Ok(())
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

    /// The prebuilt Forgejo component: `FORGEJO_PLUGIN_WASM` (set by Bazel), else the output of
    /// `cargo build --target wasm32-wasip2 -p forgejo-plugin`. Tests don't build it themselves, see
    /// `docs/development.md`.
    fn forgejo_component() -> PathBuf {
        let path = std::env::var_os("FORGEJO_PLUGIN_WASM")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/wasm32-wasip2/debug/forgejo_plugin.wasm")
            });
        assert!(
            path.exists(),
            "missing {}: run `cargo build --target wasm32-wasip2 -p forgejo-plugin` first",
            path.display()
        );
        path
    }

    fn serve_config(
        pool: &sqlx::PgPool,
        dir: &std::path::Path,
        tls: Option<&str>,
    ) -> InstanceConfig {
        let options = pool.connect_options();
        std::fs::write(dir.join("db-password"), "postgres\n").unwrap();
        let tls = tls.unwrap_or_default();
        let plugin = forgejo_component();
        let yaml = format!(
            "plugins:\n  forgejo:\n    path: {}\ndatabase:\n  host: {}\n  port: {}\n  database: {}\n  user: {}\n  passwordFile: {}/db-password\n  schema: public\n{tls}",
            plugin.display(),
            options.get_host(),
            options.get_port(),
            options.get_database().unwrap(),
            options.get_username(),
            dir.display(),
        );
        InstanceConfig::from_yaml_str(&yaml).unwrap()
    }

    #[tokio::test]
    async fn serve_runs_the_agent_port_next_to_the_ui_port() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
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
        let control_plane = tokio::spawn(serve(
            cfg,
            UnconfiguredBackend,
            http,
            agent,
            internal,
            async {
                let _ = stopped.await;
            },
        ));

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

    #[tokio::test]
    async fn serve_requires_agent_tls_configuration() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let dir = tempfile::tempdir()?;
        let cfg = serve_config(&pool, dir.path(), None);
        let listeners = [
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
        ];
        let [http, agent, internal] = listeners;
        let err = serve(
            cfg,
            UnconfiguredBackend,
            http,
            agent,
            internal,
            std::future::pending(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("agent.tls"), "{err}");
        Ok(())
    }

    async fn get_dev_sa_client() -> Client {
        let token_out = std::process::Command::new("kubectl")
            .args([
                "create",
                "token",
                "dark-factory-control-plane",
                "-n",
                "dark-factory-dev",
            ])
            .output()
            .expect("kubectl create token failed");
        assert!(token_out.status.success());
        let token = String::from_utf8(token_out.stdout)
            .unwrap()
            .trim()
            .to_string();

        let mut config = kube::Config::infer().await.unwrap();
        config.auth_info.client_certificate = None;
        config.auth_info.client_certificate_data = None;
        config.auth_info.client_key = None;
        config.auth_info.client_key_data = None;
        config.auth_info.token = Some(secrecy::SecretString::from(token));
        Client::try_from(config).unwrap()
    }

    // Dev-cluster tests (task 4.2): ignored by default, need `kubectl`, a KUBECONFIG pointing at
    // the dev cluster with the chart installed in `dark-factory-dev`, and a PostgreSQL container.

    #[tokio::test]
    #[ignore = "requires dev cluster with RBAC"]
    async fn serve_startup_check_refuses_to_start_with_missing_role() -> anyhow::Result<()> {
        let sa_client = get_dev_sa_client().await;
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let dir = tempfile::tempdir()?;
        let pki = df_agent_channel::testing::TestPki::new().map_err(anyhow::Error::from_boxed)?;
        let mut cfg = serve_config(&pool, dir.path(), Some(&agent_tls_yaml(&pki, dir.path())?));
        // The chart's Roles and ClusterRoles only cover `dark-factory-dev-sandboxes`.
        cfg.sandbox.provisioning.namespace = "default".to_string();
        cfg.sandbox.provisioning.instance_namespace = "dark-factory-dev".to_string();

        let [http, agent, internal] = bind_three().await?;
        let err = serve(
            cfg,
            KubernetesBackend { client: sa_client },
            http,
            agent,
            internal,
            std::future::pending(),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(
                err.downcast_ref::<df_k8s::StartupCheckError>(),
                Some(df_k8s::StartupCheckError::MissingPermissions { .. })
            ),
            "expected missing permissions, got: {err:#}"
        );
        assert!(
            format!("{err:#}").starts_with("startup check failed"),
            "{err:#}"
        );
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires dev cluster with RBAC"]
    async fn serve_starts_with_the_charts_rbac() -> anyhow::Result<()> {
        let sa_client = get_dev_sa_client().await;
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let dir = tempfile::tempdir()?;
        let pki = df_agent_channel::testing::TestPki::new().map_err(anyhow::Error::from_boxed)?;
        let mut cfg = serve_config(&pool, dir.path(), Some(&agent_tls_yaml(&pki, dir.path())?));
        cfg.sandbox.provisioning.namespace = "dark-factory-dev-sandboxes".to_string();
        cfg.sandbox.provisioning.instance_namespace = "dark-factory-dev".to_string();

        let [http, agent, internal] = bind_three().await?;
        let http_addr = http.local_addr()?;
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let control_plane = tokio::spawn(serve(
            cfg,
            KubernetesBackend { client: sa_client },
            http,
            agent,
            internal,
            async {
                let _ = stopped.await;
            },
        ));

        // The health port answers only once `serve` got past the startup check; if `serve` had
        // failed, the task would already have finished with an error.
        let mut ui = TcpStream::connect(http_addr).await?;
        ui.write_all(b"GET /healthz HTTP/1.1\r\n\r\n").await?;
        let mut buf = [0_u8; 64];
        let n = ui.read(&mut buf).await?;
        assert!(std::str::from_utf8(&buf[..n])?.starts_with("HTTP/1.1 200 OK"));
        assert!(!control_plane.is_finished(), "serve ended early");

        stop.send(()).ok();
        control_plane.await??;
        Ok(())
    }

    async fn bind_three() -> anyhow::Result<[TcpListener; 3]> {
        Ok([
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
            TcpListener::bind("127.0.0.1:0").await?,
        ])
    }

    fn agent_tls_yaml(
        pki: &df_agent_channel::testing::TestPki,
        dir: &std::path::Path,
    ) -> anyhow::Result<String> {
        let files = pki.write_agent_tls(dir)?;
        Ok(format!(
            "agent:\n  tls:\n    certFile: {}\n    keyFile: {}\n    clientCaFile: {}\n",
            files.cert_file.display(),
            files.key_file.display(),
            files.client_ca_file.display(),
        ))
    }
}
