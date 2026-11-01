//! Tests of the control plane side that need PostgreSQL (testcontainers) and mutual TLS.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use df_agent_channel::testing::TestPki;
use df_agent_channel::{TlsReloader, mtls_incoming};
use df_core::config::AgentTlsConfig;
use df_core::{CredentialRef, SecretError, SecretStore};
use df_db::{Notification, Notifier, PgPool};
use df_proto::gateway::v1::gateway_policy_client::GatewayPolicyClient;
use df_proto::gateway::v1::{PolicyUpdate, PolicyWatchRequest};
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::Streaming;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use uuid::Uuid;

use super::snapshot::{self, GitSettings};
use super::{PolicyService, serve_internal};
use crate::link::{GrpcConnector, MtlsConnector};

const CONTROL_PLANE: &str = "df-control-plane";
const GATEWAY: &str = "df-gateway";

/// A secret store with one value for every reference; the value can be changed.
pub(crate) struct TestSecrets(Mutex<String>);

impl TestSecrets {
    pub(crate) fn new(value: &str) -> Self {
        Self(Mutex::new(value.into()))
    }

    fn set(&self, value: &str) {
        *self.0.lock().unwrap() = value.into();
    }
}

#[async_trait::async_trait]
impl SecretStore for TestSecrets {
    async fn get(&self, _reference: &CredentialRef) -> Result<SecretString, SecretError> {
        Ok(SecretString::from(self.0.lock().unwrap().clone()))
    }
}

pub(crate) fn git_settings() -> GitSettings {
    GitSettings {
        username: "dark-factory".into(),
        commit_name: "Dark Factory".into(),
        commit_email: "factory@example.test".into(),
        allow_plain_http: false,
    }
}

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
        let service = PolicyService::new(
            pool.clone(),
            notifier,
            Arc::new(TestSecrets::new("token")),
            git_settings(),
            stop.clone(),
        )
        .debounce(Duration::from_millis(10));
        let served = stop.clone();
        let task = tokio::spawn(async move {
            serve_internal(service, incoming, Some(GATEWAY.into()), served)
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
async fn snapshot_has_the_git_route_of_an_active_run_and_the_current_forge_credential() -> Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let credential = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO credential_refs (id, name, secret_name, secret_key)
         VALUES ($1, 'forge', 'df-forge', 'token')",
    )
    .bind(credential)
    .execute(&pool)
    .await?;
    sqlx::query(
        "UPDATE projects SET forge_credential_id = $1, base_branch = 'trunk' WHERE id = $2",
    )
    .bind(credential)
    .bind(seed.project)
    .execute(&pool)
    .await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let second = insert_task(&pool, seed.project, seed.persona, 2).await?;
    insert_run(&pool, second, 1, "running", Some("10.1.0.8")).await?;
    let secrets = TestSecrets::new("token-1");

    let first = snapshot::build(&pool, &secrets, &git_settings()).await?;
    let snapshot = first.snapshot.unwrap();
    let route = snapshot.sandboxes[0].git.clone().unwrap();
    assert_eq!(route.upstream_url, "https://example.test/r.git");
    assert_eq!(route.base_branch, "refs/heads/trunk");
    assert_eq!(route.task_branch, "refs/heads/factory/task/DF-1");
    assert_eq!(route.wip_branch, "refs/heads/factory/wip/DF-1");
    assert_eq!(route.credential_ref, credential.to_string());
    assert_eq!(
        snapshot.sandboxes[1].git.as_ref().unwrap().credential_ref,
        route.credential_ref
    );
    // Both runs share one credential.
    assert_eq!(snapshot.forge_credentials.len(), 1);
    let forge = &snapshot.forge_credentials[&route.credential_ref];
    assert_eq!(
        (forge.username.as_str(), forge.password.as_str()),
        ("dark-factory", "token-1")
    );

    secrets.set("token-2");
    let second = snapshot::build(&pool, &secrets, &git_settings()).await?;
    let credentials = second.snapshot.unwrap().forge_credentials;
    assert_eq!(credentials[&route.credential_ref].password, "token-2");
    Ok(())
}

#[tokio::test]
async fn a_task_entry_reaches_only_the_runs_of_its_task() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let other = insert_task(&pool, seed.project, seed.persona, 2).await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    insert_run(&pool, other, 1, "running", Some("10.1.0.8")).await?;
    let entry = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO egress_allowlist (id, scope, task_id, kind, host, port)
         VALUES ($1, 'task', $2, 'http-host', 'pypi.org', 443)",
    )
    .bind(entry)
    .bind(seed.task)
    .execute(&pool)
    .await?;

    let update = snapshot::build(&pool, &TestSecrets::new("t"), &git_settings()).await?;

    let sandboxes = update.snapshot.unwrap().sandboxes;
    let hosts = |key: &str| -> Vec<(String, u32, String, String)> {
        sandboxes
            .iter()
            .find(|sandbox| sandbox.task_key == key)
            .unwrap()
            .hosts
            .iter()
            .map(|h| {
                (
                    h.pattern.clone(),
                    h.port,
                    h.scope.clone(),
                    h.entry_id.clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        hosts("DF-1"),
        [(
            "pypi.org".to_string(),
            443,
            "task".to_string(),
            entry.to_string()
        )]
    );
    assert!(hosts("DF-2").is_empty());
    Ok(())
}

#[tokio::test]
async fn a_changed_forge_secret_reaches_the_watch_after_the_refresh_period() -> Result<()> {
    use df_proto::gateway::v1::gateway_policy_server::GatewayPolicy;
    use futures::StreamExt;

    const PERIOD: Duration = Duration::from_secs(60);
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let credential = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO credential_refs (id, name, secret_name, secret_key)
         VALUES ($1, 'forge', 'df-forge', 'token')",
    )
    .bind(credential)
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE projects SET forge_credential_id = $1 WHERE id = $2")
        .bind(credential)
        .bind(seed.project)
        .execute(&pool)
        .await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let secrets = Arc::new(TestSecrets::new("token-1"));
    let service = PolicyService::new(
        pool.clone(),
        Arc::new(Notifier::start(&pool).await?),
        Arc::clone(&secrets) as Arc<dyn SecretStore>,
        git_settings(),
        CancellationToken::new(),
    )
    .refresh(PERIOD);
    let mut updates = service
        .policy_watch(tonic::Request::new(PolicyWatchRequest {
            gateway_pod: "gw-0".into(),
        }))
        .await?
        .into_inner();
    let password = |update: &PolicyUpdate| {
        update.snapshot.as_ref().map(|snapshot| {
            snapshot.forge_credentials[&credential.to_string()]
                .password
                .clone()
        })
    };
    let first = updates
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("no update"))??;
    assert_eq!(password(&first).as_deref(), Some("token-1"));

    secrets.set("token-2");
    // The period started when the first update was built; no I/O since then.
    tokio::time::pause();
    let early = tokio::time::timeout(
        PERIOD.saturating_sub(Duration::from_secs(10)),
        updates.next(),
    )
    .await;
    assert!(early.is_err(), "an update before the refresh period");
    tokio::time::advance(Duration::from_secs(20)).await;
    tokio::time::resume();
    let second = tokio::time::timeout(Duration::from_secs(5), updates.next())
        .await?
        .ok_or_else(|| anyhow::anyhow!("the stream ended"))??;
    assert_eq!(password(&second).as_deref(), Some("token-2"));
    Ok(())
}

#[tokio::test]
async fn record_push_is_accepted_only_from_the_gateway_certificate() -> Result<()> {
    use df_proto::gateway::v1::gateway_git_client::GatewayGitClient;
    use df_proto::gateway::v1::{PushRecord, PushVerdict, RefUpdate};

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let record = |push_id: Uuid| PushRecord {
        push_id: push_id.to_string(),
        run_id: run.to_string(),
        gateway_pod: "gw-0".into(),
        refs: vec![RefUpdate {
            name: "refs/heads/factory/task/DF-1".into(),
            old_sha: "0".repeat(40),
            new_sha: "a".repeat(40),
            verdict: PushVerdict::Accepted.into(),
            reason: String::new(),
        }],
        verdict: PushVerdict::Accepted.into(),
        bytes: 7,
        duration: None,
    };

    let (files, _dir) = gateway_credentials(&pki)?;
    let channel = gateway_connector(files)
        .connect(&replica.addr.to_string())
        .await?;
    let accepted = Uuid::now_v7();
    GatewayGitClient::new(channel)
        .record_push(record(accepted))
        .await?;

    let pod = pki
        .pod_client("spiffe://dark-factory/ns/sb/pod/run/uid")
        .map_err(anyhow::Error::from_boxed)?;
    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(&pki.ca_pem))
        .identity(Identity::from_pem(pod.cert_pem, pod.key_pem))
        .domain_name(CONTROL_PLANE);
    let channel = Endpoint::from_shared(format!("https://{}", replica.addr))?
        .tls_config(tls)?
        .connect()
        .await?;
    let rejected = Uuid::now_v7();
    let status = GatewayGitClient::new(channel)
        .record_push(record(rejected))
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status}");

    let stored: Vec<Uuid> = sqlx::query_scalar("SELECT push_id FROM git_pushes")
        .fetch_all(&pool)
        .await?;
    assert_eq!(stored, [accepted]);
    replica.stop().await;
    Ok(())
}

#[tokio::test]
async fn a_reported_denial_raises_one_network_access_request() -> Result<()> {
    use df_proto::gateway::v1::Denial;
    use df_proto::gateway::v1::gateway_events_client::GatewayEventsClient;

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let (files, _dir) = gateway_credentials(&pki)?;
    let channel = gateway_connector(files)
        .connect(&replica.addr.to_string())
        .await?;
    let mut client = GatewayEventsClient::new(channel);
    let denial = |run_id: String, port| Denial {
        run_id,
        host: "statsig.anthropic.com".into(),
        port,
        at: None,
    };

    client.report_denial(denial(run.to_string(), 443)).await?;
    client.report_denial(denial(run.to_string(), 443)).await?;
    let invalid = client
        .report_denial(denial(run.to_string(), 0))
        .await
        .unwrap_err();
    let malformed = client
        .report_denial(denial("run-1".into(), 443))
        .await
        .unwrap_err();

    assert_eq!(invalid.code(), tonic::Code::InvalidArgument, "{invalid}");
    assert_eq!(
        malformed.code(),
        tonic::Code::InvalidArgument,
        "{malformed}"
    );
    let stored: Vec<(String, String, bool, Option<Uuid>)> = sqlx::query_as(
        "SELECT approval_type, fingerprint, blocking, run_id FROM agent_requests
         WHERE task_id = $1 AND state = 'pending'",
    )
    .bind(seed.task)
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        stored,
        [(
            "network-access".to_owned(),
            "statsig.anthropic.com:443".to_owned(),
            false,
            None
        )]
    );
    replica.stop().await;
    Ok(())
}

#[tokio::test]
async fn a_rejected_push_without_ref_updates_is_stored() -> Result<()> {
    use df_proto::gateway::v1::gateway_git_client::GatewayGitClient;
    use df_proto::gateway::v1::{PushRecord, PushVerdict};

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let replica = Replica::start(&pool, &pki).await?;
    let (files, _dir) = gateway_credentials(&pki)?;
    let channel = gateway_connector(files)
        .connect(&replica.addr.to_string())
        .await?;
    let push_id = Uuid::now_v7();
    GatewayGitClient::new(channel)
        .record_push(PushRecord {
            push_id: push_id.to_string(),
            run_id: run.to_string(),
            gateway_pod: "gw-0".into(),
            refs: Vec::new(),
            verdict: PushVerdict::Rejected.into(),
            bytes: 3,
            duration: None,
        })
        .await?;

    let stored: Vec<(Uuid, String)> = sqlx::query_as("SELECT push_id, verdict FROM git_pushes")
        .fetch_all(&pool)
        .await?;
    assert_eq!(stored, [(push_id, "rejected".to_owned())]);
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

#[tokio::test]
async fn approving_a_network_access_request_for_the_task_reaches_the_run_snapshot() -> Result<()> {
    use df_core::{Principal, RunId, UserId};
    use df_db::network_access::{DenialOutcome, raise_from_denial};
    use df_db::requests::{ApprovalTypes, Decision, Raised, Scope, decide};

    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let raised = raise_from_denial(&pool, RunId::from_uuid(run), "pypi.org", 443).await?;
    let DenialOutcome::Raised(Raised::Request { request, new: true }) = raised else {
        anyhow::bail!("no request raised: {raised:?}");
    };
    let notifier = Notifier::start(&pool).await?;
    let mut notifications = notifier.subscribe();

    decide(
        &pool,
        &ApprovalTypes::builtin(),
        request.id,
        &Principal::human(UserId::new(), "Peter"),
        &["board-member".to_string()],
        Decision::Approve(Scope::Task),
    )
    .await?;

    tokio::time::timeout(Duration::from_secs(5), async {
        while notifications.recv().await? != Notification::EgressPolicy {}
        anyhow::Ok(())
    })
    .await??;
    let update = snapshot::build(&pool, &TestSecrets::new("t"), &git_settings()).await?;
    let sandboxes = sandboxes(&update);
    let [sandbox] = sandboxes else {
        anyhow::bail!("expected one sandbox: {update:?}");
    };
    let hosts: Vec<_> = sandbox
        .hosts
        .iter()
        .map(|h| (h.pattern.as_str(), h.port, h.scope.as_str()))
        .collect();
    assert_eq!(hosts, [("pypi.org", 443, "task")]);
    Ok(())
}
