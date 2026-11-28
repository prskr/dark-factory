//! MCP servers through the gateway (`just e2e-local`; `iter4-mcp-gateway`, `mcp-endpoint` and
//! `mcp-servers`): the agent lists and calls a tool through the relay `ext`, a call to a server
//! that is registered but not enabled for the project is rejected without reaching it, the call
//! records show in the run view, and no sandbox holds the credential. Needs the environment of
//! `E2eEnv`, so the test is ignored by default.

use anyhow::{Context, Result};
use df_core::mcp::McpCallOutcome;
use df_e2e::fixture::World;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db, fixture};

/// mcp-endpoint, scenarios "Successful call" and "Server of another project"; mcp-servers,
/// "Enable for a project" and "Run with two calls": the docs server is enabled for the project
/// and answers, the other one is registered but not enabled and is rejected at the gateway. The
/// names are the test's own (at most 20 characters) and reach the script `mcp.yaml` as the
/// markers `[docs=…]` and `[other=…]` of the task description.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_tool_of_an_enabled_server_is_called_through_the_gateway_and_one_of_another_server_is_rejected()
-> Result<()> {
    fixture::run(
        "a_tool_of_an_enabled_server_is_called_through_the_gateway_and_one_of_another_server_is_rejected",
        |world| async move {
            let (docs, other) = (world.unique("mcpd"), world.unique("mcpo"));
            let ids = db::register_mcp_servers(&world.pool, &[&docs, &other]).await?;
            let docs_id = *ids.first().context("the id of the docs server")?;
            db::enable_mcp_servers(&world.pool, data::MCP_PROJECT_KEY, &[docs_id]).await?;
            let result = run_task(&world, &docs, &other).await;
            // The servers are instance-wide: leave nothing behind for the next test or run.
            db::remove_mcp_servers(&world.pool, &[&docs, &other]).await?;
            result
        },
    )
    .await
}

async fn run_task(world: &World, docs: &str, other: &str) -> Result<()> {
    let key = world
        .session
        .create_task(
            &world.env,
            data::MCP_PROJECT_KEY,
            &format!("[mcp] [docs={docs}] [other={other}] Use the MCP servers."),
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
            (docs.to_owned(), "whoami".to_owned(), McpCallOutcome::Ok),
            (
                other.to_owned(),
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
        &format!("{docs}__whoami"),
        &format!("{other}__whoami"),
        "rejected",
    ] {
        world
            .session
            .expect_region_text("#runs", text, UI_TIMEOUT)
            .await?;
    }

    // What the agent saw: the enabled server answered, with its credential redacted, and the
    // rejected call says why. The transcript is written asynchronously, so wait for both events
    // before asserting what is not in it.
    let answered = format!("{docs} reached; credential line: Authorization: Bearer [redacted]");
    for (what, text) in [
        ("the answer of the docs server", answered.as_str()),
        (
            "the refusal of the other server",
            "is not available in this run",
        ),
    ] {
        eventually(&format!("{what} in the transcript"), UI_TIMEOUT, || async {
            let found = db::transcript_events_containing(&world.pool, &key, text).await?;
            Ok((found >= 1).then_some(()))
        })
        .await?;
    }
    assert_eq!(
        db::transcript_events_containing(&world.pool, &key, &format!("{other} reached")).await?,
        0,
        "the server that is not enabled was never reached"
    );
    // The sandbox holds no credential: it is nowhere in what the agent and its harness wrote.
    assert_eq!(
        db::transcript_events_containing(&world.pool, &key, data::MCP_TOKEN).await?,
        0,
        "the credential never reaches the transcript"
    );
    Ok(())
}
