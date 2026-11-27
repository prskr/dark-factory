//! MCP servers through the gateway (`just e2e-local`; `iter4-mcp-gateway`, `mcp-endpoint` and
//! `mcp-servers`): the agent lists and calls a tool through the relay `ext`, a call to a server
//! that is registered but not enabled for the project is rejected without reaching it, the call
//! records show in the run view, and no sandbox holds the credential. Needs the environment of
//! `E2eEnv`, so the test is ignored by default.

use anyhow::{Context, Result};
use df_core::mcp::McpCallOutcome;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db, fixture};

/// The two servers the script `mcp.yaml` calls; the test registers them.
const DOCS: &str = "e2e-docs";
const OTHER: &str = "e2e-other";

/// mcp-endpoint, scenarios "Successful call" and "Server of another project"; mcp-servers,
/// "Enable for a project" and "Run with two calls": `e2e-docs` is enabled for the project and
/// answers, `e2e-other` is registered but not enabled and is rejected at the gateway.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_tool_of_an_enabled_server_is_called_through_the_gateway_and_one_of_another_server_is_rejected()
-> Result<()> {
    fixture::run(
        "a_tool_of_an_enabled_server_is_called_through_the_gateway_and_one_of_another_server_is_rejected",
        |world| async move {
            let ids = db::register_mcp_servers(&world.pool, &[DOCS, OTHER]).await?;
            let docs = *ids.first().context("the id of e2e-docs")?;
            db::enable_mcp_servers(&world.pool, data::MCP_PROJECT_KEY, &[docs]).await?;
            let result = run_task(&world).await;
            // The servers are instance-wide: leave nothing behind for the next test or run.
            db::remove_mcp_servers(&world.pool, &[DOCS, OTHER]).await?;
            result
        },
    )
    .await
}

async fn run_task(world: &fixture::World) -> Result<()> {
    let key = world
        .session
        .create_task(
            &world.env,
            data::MCP_PROJECT_KEY,
            "[mcp] Use the MCP servers.",
            None,
        )
        .await?;

    // One record per call: the enabled server answered, the other one was rejected.
    let calls = eventually("the two MCP call records", RUN_TIMEOUT, || async {
        Ok(Some(db::mcp_calls(&world.pool, &key).await?).filter(|calls| calls.len() >= 2))
    })
    .await?;
    assert_eq!(
        calls,
        [
            (DOCS.to_owned(), "whoami".to_owned(), McpCallOutcome::Ok),
            (
                OTHER.to_owned(),
                "whoami".to_owned(),
                McpCallOutcome::Rejected
            ),
        ]
    );

    // The run view lists both calls, with their outcomes (the heading is upper case by style).
    world.session.open_task(&world.env, &key).await?;
    world.session.open_runs().await?;
    for text in [
        "TOOL CALLS (MCP)",
        "e2e-docs__whoami",
        "e2e-other__whoami",
        "rejected",
    ] {
        world
            .session
            .expect_region_text("#runs", text, UI_TIMEOUT)
            .await?;
    }

    // What the agent saw: the enabled server answered, with its credential redacted; the other
    // server was never reached, the gateway refused the call.
    let token = data::MCP_TOKEN;
    let answered = "e2e-docs reached; credential line: Authorization: Bearer [redacted]";
    assert!(
        db::transcript_events_containing(&world.pool, &key, answered).await? >= 1,
        "the transcript holds the answer of e2e-docs with the credential redacted"
    );
    assert_eq!(
        db::transcript_events_containing(&world.pool, &key, "e2e-other reached").await?,
        0,
        "the server that is not enabled was never reached"
    );
    assert!(
        db::transcript_events_containing(&world.pool, &key, "is not available in this run").await?
            >= 1,
        "the rejected call says why"
    );
    // The sandbox holds no credential: it is nowhere in what the agent and its harness wrote.
    assert_eq!(
        db::transcript_events_containing(&world.pool, &key, token).await?,
        0,
        "the credential never reaches the transcript"
    );
    Ok(())
}
