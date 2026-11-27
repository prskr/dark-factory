//! The MCP part of the snapshot and `GatewayMcp.Report`.

use anyhow::Result;
use df_core::mcp::{McpCallOutcome, McpCredentialScheme};
use df_core::{Principal, SecretError, SecretKeyRef, SecretStore, UserId};
use df_db::catalog::{Actor, CredentialRefInput, save_credential_ref};
use df_db::mcp_servers::{
    McpServerInput, list_mcp_calls_of_runs, save_mcp_server, set_project_mcp_servers,
};
use df_proto::gateway::v1 as pb;
use df_proto::gateway::v1::gateway_mcp_server::GatewayMcp;
use secrecy::SecretString;
use tonic::Request;

use super::*;
use crate::mcp_calls::McpCalls;

/// Reads every Secret except `unreadable`, as `value-of-<name>`.
struct Secrets {
    unreadable: &'static str,
}

#[async_trait::async_trait]
impl SecretStore for Secrets {
    async fn get(&self, reference: &SecretKeyRef) -> Result<SecretString, SecretError> {
        if reference.name == self.unreadable {
            return Err(SecretError::NotFound(reference.clone()));
        }
        Ok(SecretString::from(format!("value-of-{}", reference.name)))
    }

    async fn get_or_create(
        &self,
        reference: &SecretKeyRef,
        _labels: &BTreeMap<String, String>,
    ) -> Result<SecretString, SecretError> {
        self.get(reference).await
    }

    async fn get_or_create_with(
        &self,
        reference: &SecretKeyRef,
        _labels: &BTreeMap<String, String>,
        _value: &SecretString,
    ) -> Result<SecretString, SecretError> {
        self.get(reference).await
    }

    async fn replace(
        &self,
        reference: &SecretKeyRef,
        _value: &SecretString,
    ) -> Result<(), SecretError> {
        Err(SecretError::NotFound(reference.clone()))
    }
}

/// Registers server `name` with a credential reference to Secret `mcp-<name>`.
async fn register(pool: &PgPool, name: &str) -> Result<Uuid> {
    let principal = Principal::human(UserId::new(), "Peter");
    let roles = vec!["admin".to_string()];
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let credential = save_credential_ref(
        pool,
        actor,
        None,
        &CredentialRefInput {
            name: format!("{name}-key"),
            secret_name: format!("mcp-{name}"),
            secret_key: "key".into(),
        },
    )
    .await?;
    let server = save_mcp_server(
        pool,
        actor,
        None,
        &McpServerInput {
            name: name.into(),
            url: format!("https://{name}.example.test/mcp?file=abc"),
            credential_ref_id: credential.id.as_uuid(),
            scheme: McpCredentialScheme::Query {
                parameter: "token".into(),
            },
            private_network: true,
        },
    )
    .await?;
    Ok(server.id)
}

async fn enable(pool: &PgPool, project: Uuid, servers: &[Uuid]) -> Result<()> {
    let principal = Principal::human(UserId::new(), "Peter");
    let roles = vec!["admin".to_string()];
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    set_project_mcp_servers(pool, actor, project, servers).await?;
    Ok(())
}

#[tokio::test]
async fn snapshot_carries_the_servers_of_active_runs_with_their_credentials_and_run_entries()
-> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let context7 = register(&pool, "context7").await?;
    register(&pool, "billing").await?;
    enable(&pool, seed.project, &[context7]).await?;

    let update = snapshot::build(&pool, &Secrets { unreadable: "" }, &git_settings()).await?;

    let snapshot = update.snapshot.unwrap();
    assert_eq!(snapshot.sandboxes[0].mcp_server_ids, [context7.to_string()]);
    assert_eq!(
        snapshot.mcp_servers.len(),
        1,
        "billing is enabled for no project"
    );
    let server = &snapshot.mcp_servers[&context7.to_string()];
    assert_eq!(server.name, "context7");
    assert_eq!(server.url, "https://context7.example.test/mcp?file=abc");
    assert!(server.private_network);
    assert_eq!(server.config_version, 1);
    assert_eq!(
        server.credential_scheme,
        Some(pb::mcp_server::CredentialScheme::Query(
            pb::McpQueryScheme {
                parameter: "token".into()
            }
        ))
    );
    assert_eq!(
        snapshot.provider_credentials[&server.credential_ref].value,
        "value-of-mcp-context7"
    );
    assert!(
        !snapshot
            .provider_credentials
            .values()
            .any(|c| c.value.contains("billing")),
        "the credential of a server no active run uses stays out"
    );
    Ok(())
}

#[tokio::test]
async fn a_server_of_a_project_without_active_runs_is_not_in_the_snapshot() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    insert_run(&pool, seed.task, 1, "succeeded", Some("10.1.0.7")).await?;
    let server = register(&pool, "context7").await?;
    enable(&pool, seed.project, &[server]).await?;

    let snapshot = snapshot::build(&pool, &Secrets { unreadable: "" }, &git_settings())
        .await?
        .snapshot
        .unwrap();

    assert!(snapshot.mcp_servers.is_empty());
    assert!(snapshot.sandboxes.is_empty());
    Ok(())
}

#[tokio::test]
async fn an_unreadable_mcp_credential_leaves_the_server_without_credential_and_others_intact()
-> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let broken = register(&pool, "wiki").await?;
    let working = register(&pool, "context7").await?;
    enable(&pool, seed.project, &[broken, working]).await?;

    let snapshot = snapshot::build(
        &pool,
        &Secrets {
            unreadable: "mcp-wiki",
        },
        &git_settings(),
    )
    .await?
    .snapshot
    .unwrap();

    assert_eq!(snapshot.mcp_servers.len(), 2);
    let wiki = &snapshot.mcp_servers[&broken.to_string()];
    let context7 = &snapshot.mcp_servers[&working.to_string()];
    assert!(
        !snapshot
            .provider_credentials
            .contains_key(&wiki.credential_ref)
    );
    assert_eq!(
        snapshot.provider_credentials[&context7.credential_ref].value,
        "value-of-mcp-context7"
    );
    assert_eq!(snapshot.sandboxes[0].mcp_server_ids.len(), 2);
    Ok(())
}

fn record(run: Uuid, call: Uuid, tool: &str) -> pb::McpCallRecord {
    pb::McpCallRecord {
        call_id: call.to_string(),
        run_id: run.to_string(),
        started_at: Some(df_proto::pbjson_types::Timestamp {
            seconds: 1_800_000_000,
            nanos: 0,
        }),
        server_name: "context7".into(),
        tool: tool.into(),
        outcome: pb::McpCallOutcome::Ok.into(),
        duration_ms: 1200,
        request_bytes: 400,
        response_bytes: 3100,
    }
}

#[tokio::test]
async fn reported_calls_are_stored_once_and_records_of_unknown_runs_are_dropped() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let seed = seed(&pool).await?;
    let run = insert_run(&pool, seed.task, 1, "running", Some("10.1.0.7")).await?;
    let service = McpCalls::new(pool.clone());
    let call = Uuid::now_v7();
    let batch = pb::McpCallBatch {
        gateway_pod: "gateway-0".into(),
        records: vec![
            record(run, call, "resolve-library-id"),
            record(run, call, "resolve-library-id"),
            record(Uuid::now_v7(), Uuid::now_v7(), "lost"),
            pb::McpCallRecord {
                outcome: 0,
                ..record(run, Uuid::now_v7(), "invalid")
            },
        ],
    };

    service.report(Request::new(batch.clone())).await?;
    // A retried batch changes nothing.
    service.report(Request::new(batch)).await?;

    let stored = list_mcp_calls_of_runs(&pool, &[run]).await?;
    let calls = &stored[&run];
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "resolve-library-id");
    assert_eq!(calls[0].outcome, McpCallOutcome::Ok);
    assert_eq!(
        (
            calls[0].duration_ms,
            calls[0].request_bytes,
            calls[0].response_bytes
        ),
        (1200, 400, 3100)
    );
    Ok(())
}
