//! Tests of the control plane side that need PostgreSQL (testcontainers) and mutual TLS.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use df_agent_channel::testing::TestPki;
use df_agent_channel::{TlsReloader, mtls_incoming};
use df_core::config::AgentTlsConfig;
use df_db::{Notification, Notifier, PgPool};
use df_proto::gateway::v1::gateway_policy_client::GatewayPolicyClient;
use df_proto::gateway::v1::{PolicyUpdate, PolicyWatchRequest};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::Streaming;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use uuid::Uuid;

use super::{PolicyService, serve_policy};
use crate::link::{GrpcConnector, MtlsConnector};

const CONTROL_PLANE: &str = "df-control-plane";
const GATEWAY: &str = "df-gateway";

pub(crate) struct Seed {
    pub project: Uuid,
    pub persona: Uuid,
    pub task: Uuid,
}

/// A project `DF` with task `DF-1`.
pub(crate) async fn seed(pool: &PgPool) -> Result<Seed> {
    let (project, persona) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ($1, 'DF', 'Dark Factory', 'https://example.test/r.git', 'img:latest')",
    )
    .bind(project)
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO personas (id, name, system_prompt) VALUES ($1, 'Linus', 'x')")
        .bind(persona)
        .execute(pool)
        .await?;
    let task = insert_task(pool, project, persona, 1).await?;
    Ok(Seed {
        project,
        persona,
        task,
    })
}

/// Task `DF-<number>`.
pub(crate) async fn insert_task(
    pool: &PgPool,
    project: Uuid,
    persona: Uuid,
    number: i64,
) -> Result<Uuid> {
    let task = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id)
         VALUES ($1, $2, $3, 'DF-' || $3::text, 't', 'user', $4)",
    )
    .bind(task)
    .bind(project)
    .bind(number)
    .bind(persona)
    .execute(pool)
    .await?;
    Ok(task)
}

pub(crate) async fn insert_run(
    pool: &PgPool,
    task: Uuid,
    number: i64,
    state: &str,
    pod_ip: Option<&str>,
) -> Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(task)
    .bind(number)
    .bind(state)
    .bind(pod_ip)
    .execute(pool)
    .await?;
    Ok(id)
}

pub(crate) async fn insert_host(
    pool: &PgPool,
    project: Option<Uuid>,
    host: &str,
    port: i32,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO egress_allowlist (id, scope, project_id, kind, host, port)
         VALUES ($1, $2, $3, 'http-host', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(if project.is_some() {
        "project"
    } else {
        "instance"
    })
    .bind(project)
    .bind(host)
    .bind(port)
    .execute(pool)
    .await?;
    Ok(())
}

/// A control plane replica serving `PolicyWatch` with mTLS on a loopback port.
pub(crate) struct Replica {
    pub addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Replica {
    pub(super) async fn start(pool: &PgPool, pki: &TestPki) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let files = pki
            .write_peer_tls(dir.path(), CONTROL_PLANE)
            .map_err(anyhow::Error::from_boxed)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let incoming = mtls_incoming(listener, TlsReloader::load(&files).await?);
        let stop = CancellationToken::new();
        // Each replica listens on its own connection, like separate pods.
        let notifier = Arc::new(Notifier::start(pool).await?);
        let service = PolicyService::new(pool.clone(), notifier, stop.clone())
            .debounce(Duration::from_millis(10));
        let served = stop.clone();
        let task = tokio::spawn(async move {
            serve_policy(service, incoming, Some(GATEWAY.into()), served)
                .await
                .ok();
        });
        Ok(Self {
            addr,
            stop,
            task,
            _dir: dir,
        })
    }

    pub(super) async fn stop(self) {
        self.stop.cancel();
        self.task.await.ok();
    }
}

/// Credentials of a gateway pod signed by the CA of `pki`.
pub(crate) fn gateway_credentials(pki: &TestPki) -> Result<(AgentTlsConfig, tempfile::TempDir)> {
    let dir = tempfile::tempdir()?;
    let files = pki
        .write_peer_tls(dir.path(), GATEWAY)
        .map_err(anyhow::Error::from_boxed)?;
    Ok((files, dir))
}

pub(crate) fn gateway_connector(files: AgentTlsConfig) -> MtlsConnector {
    MtlsConnector::new(files, CONTROL_PLANE)
}

async fn watch(channel: Channel) -> Result<Streaming<PolicyUpdate>, tonic::Status> {
    Ok(GatewayPolicyClient::new(channel)
        .policy_watch(PolicyWatchRequest {
            gateway_pod: "gw-0".into(),
        })
        .await?
        .into_inner())
}

async fn next(updates: &mut Streaming<PolicyUpdate>) -> Result<PolicyUpdate> {
    let update = tokio::time::timeout(Duration::from_secs(5), updates.message())
        .await??
        .ok_or_else(|| anyhow::anyhow!("the stream ended"))?;
    Ok(update)
}

fn sandboxes(update: &PolicyUpdate) -> &[df_proto::gateway::v1::SandboxEntry] {
    update
        .snapshot
        .as_ref()
        .map_or(&[], |snapshot| &snapshot.sandboxes)
}

#[tokio::test]
async fn gateway_connects_and_receives_every_active_run_with_a_pod_ip() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let active = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let second = insert_task(&pool, seed.project, seed.persona, 2).await?;
    insert_run(&pool, second, 1, "provisioning", None).await?;
    insert_run(&pool, seed.task, 2, "succeeded", Some("10.1.0.9")).await?;
    insert_host(&pool, None, "api.anthropic.com", 443).await?;
    insert_host(&pool, Some(seed.project), "*.crates.io", 443).await?;

    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let (files, _dir) = gateway_credentials(&pki)?;
    let channel = gateway_connector(files)
        .connect(&replica.addr.to_string())
        .await?;
    let mut updates = watch(channel).await?;

    let first = next(&mut updates).await?;
    let [sandbox] = sandboxes(&first) else {
        anyhow::bail!("expected one sandbox: {first:?}");
    };
    assert_eq!(sandbox.pod_ip, "10.1.0.7");
    assert_eq!(sandbox.run_id, active.to_string());
    assert_eq!(
        (sandbox.task_key.as_str(), sandbox.project_key.as_str()),
        ("DF-1", "DF")
    );
    let hosts: Vec<_> = sandbox.hosts.iter().map(|h| h.pattern.as_str()).collect();
    assert_eq!(hosts, ["api.anthropic.com", "*.crates.io"]);
    assert!(first.version > 0);
    replica.stop().await;
    Ok(())
}

#[tokio::test]
async fn clients_without_a_valid_instance_certificate_are_rejected() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let url = format!("https://{}", replica.addr);
    let ca = Certificate::from_pem(&pki.ca_pem);
    let connect = |tls: ClientTlsConfig| {
        let url = url.clone();
        async move {
            let channel = Endpoint::from_shared(url)?
                .tls_config(tls.domain_name(CONTROL_PLANE))?
                .connect()
                .await?;
            watch(channel)
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
        }
    };

    // No client certificate at all.
    let tls = ClientTlsConfig::new().ca_certificate(ca.clone());
    assert!(connect(tls).await.is_err(), "no certificate");

    // A certificate of an unrelated CA.
    let rogue = TestPki::rogue_client("spiffe://dark-factory/ns/sb/pod/run/uid")
        .map_err(anyhow::Error::from_boxed)?;
    let tls = ClientTlsConfig::new()
        .ca_certificate(ca.clone())
        .identity(Identity::from_pem(rogue.cert_pem, rogue.key_pem));
    assert!(connect(tls).await.is_err(), "rogue CA");

    // A sandbox pod's certificate chains to the instance CA but is no gateway.
    let pod = pki
        .pod_client("spiffe://dark-factory/ns/sb/pod/run/uid")
        .map_err(anyhow::Error::from_boxed)?;
    let tls = ClientTlsConfig::new()
        .ca_certificate(ca)
        .identity(Identity::from_pem(pod.cert_pem, pod.key_pem));
    let error = connect(tls).await.unwrap_err();
    let denied = error
        .downcast_ref::<tonic::Status>()
        .is_some_and(|status| status.code() == tonic::Code::PermissionDenied);
    assert!(denied, "{error:#}");
    replica.stop().await;
    Ok(())
}

#[tokio::test]
async fn ending_a_run_removes_its_pod_ip_in_the_next_update() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let (files, _dir) = gateway_credentials(&pki)?;
    let channel = gateway_connector(files)
        .connect(&replica.addr.to_string())
        .await?;
    let mut updates = watch(channel).await?;
    let first = next(&mut updates).await?;
    assert_eq!(sandboxes(&first).len(), 1);

    let task_id = df_core::TaskId::from_uuid(seed.task);
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(run)
        .execute(&pool)
        .await?;
    df_db::publish(
        &pool,
        &Notification::RunChanged {
            run_id: df_core::RunId::from_uuid(run),
            task_id,
        },
    )
    .await?;

    let second = next(&mut updates).await?;
    assert!(sandboxes(&second).is_empty(), "{second:?}");
    assert!(second.version > first.version);
    replica.stop().await;
    Ok(())
}

/// 3.2: two replicas on one database. The gateway is connected to replica B, the entry is added
/// through replica A's side of the database; B's notification reaches the gateway.
#[tokio::test]
async fn an_entry_added_on_one_replica_reaches_a_gateway_on_another_within_a_second() -> Result<()>
{
    use df_core::{Principal, UserId};
    use df_db::catalog::Actor;
    use df_db::{EntryKind, add_entry};

    use crate::policy::{Request, decide};

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica_a = Replica::start(&pool, &pki).await?;
    let replica_b = Replica::start(&pool, &pki).await?;
    let (files, _dir) = gateway_credentials(&pki)?;

    let (policy_tx, mut policy) = tokio::sync::watch::channel(None);
    let watcher = tokio::spawn(crate::watch::run(
        Arc::new(gateway_connector(files)),
        replica_b.addr.to_string(),
        "gw-0".into(),
        policy_tx,
        Duration::from_millis(50),
        Arc::default(),
    ));
    let allows = |policy: &Option<Arc<crate::policy::Policy>>| {
        decide(
            policy.as_deref(),
            "10.1.0.7".parse().unwrap(),
            Request::Connect {
                host: "pypi.org",
                port: 443,
            },
        )
        .verdict
        .is_ok()
    };
    tokio::time::timeout(Duration::from_secs(5), policy.wait_for(Option::is_some)).await??;
    assert!(!allows(&policy.borrow()));

    let principal = Principal::human(UserId::new(), "Peter");
    let roles = vec!["board-member".to_string()];
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let kind = EntryKind::HttpHost {
        host: "pypi.org".into(),
        port: 443,
    };
    add_entry(&pool, actor, Some("DF"), &kind).await?;

    tokio::time::timeout(
        Duration::from_secs(1),
        policy.wait_for(|policy| allows(policy)),
    )
    .await??;

    watcher.abort();
    replica_a.stop().await;
    replica_b.stop().await;
    Ok(())
}
