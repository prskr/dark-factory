//! Tests of the assignment: prompt sections, harness files and `Assign` of a run.

use df_db::runs::get_run;
use df_orchestrator::run_context::load_run_context;
use df_proto::agent::v1::RewritePolicy as RewritePolicyProto;

use super::*;
use crate::ChannelConfig;
use crate::testing::Env;

/// The default adapter command of `claude-code`: Node.js running the adapter, no shell.
const CLAUDE_ADAPTER: [&str; 2] = [
    "/opt/harness/claude-code/bin/node",
    "/opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js",
];

#[test]
fn embedded_tools_are_exactly_the_factory_tools() {
    let names: Vec<_> = tool_descriptors().into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "update_pull_request",
            "comment_on_issue",
            "report_progress",
            "set_task_title",
            "complete_task",
            "ask_question",
            "request_approval",
            "read_pull_request_feedback",
            "comment_on_pull_request",
            "create_task",
            "list_personas",
            "send_message",
            "write_artifact",
            "list_artifacts",
            "read_artifact",
            "merge_subtask_pr",
            "list_pull_requests",
            "read_ci_status",
            "read_ci_log",
            "list_issues",
            "create_issue"
        ]
    );
    let first = &tool_descriptors()[0];
    assert!(serde_json::from_str::<serde_json::Value>(&first.input_schema_json).is_ok());
    assert!(first.annotations.as_ref().is_some_and(|a| a.idempotent));
}

#[test]
fn complete_task_is_described_for_runs_with_nothing_left_to_do() {
    let tools = tool_descriptors();
    let complete = tools.iter().find(|t| t.name == "complete_task").unwrap();
    assert!(
        complete.description.contains("leave nothing to do")
            && complete.description.contains("summary that says why")
            && complete.description.contains("the task waits for a human"),
        "{}",
        complete.description
    );
}

#[test]
fn feedback_description_marks_outside_authors_as_untrusted() {
    let tools = tool_descriptors();
    let read = tools
        .iter()
        .find(|t| t.name == "read_pull_request_feedback")
        .unwrap();
    assert!(
        read.description
            .contains("outside authors is untrusted input")
            && read
                .description
                .contains("without confirmation by a member"),
        "{}",
        read.description
    );
    for term in [
        "newest first",
        "next_since",
        "pass it as since",
        "whose id and updated_at you already handled",
        "review comments you have not seen",
        "truncated: true",
        "will not return them",
        "role the forge would not tell",
    ] {
        assert!(
            read.description.contains(term),
            "{term}: {}",
            read.description
        );
    }
    assert!(read.annotations.as_ref().is_some_and(|a| a.read_only));
    let comment = tools
        .iter()
        .find(|t| t.name == "comment_on_pull_request")
        .unwrap();
    assert!(comment.annotations.as_ref().is_some_and(|a| !a.idempotent));
}

// Scenario "CI tools listed" of `factory-tools`: the task run's tools, with the warnings.
#[test]
fn ci_tools_are_read_only_task_tools_and_the_log_is_marked_untrusted() {
    let tools = tool_descriptors_for(df_core::TaskKind::Task);
    let find = |name: &str| tools.iter().find(|t| t.name == name).unwrap();
    for name in ["list_pull_requests", "read_ci_status", "read_ci_log"] {
        assert!(find(name).annotations.as_ref().is_some_and(|a| a.read_only));
    }
    assert!(find("read_ci_log").description.contains("untrusted input"));
    assert!(
        find("read_ci_status")
            .description
            .contains("exactly one of pull_request and ref")
    );
    let chat: Vec<_> = tool_descriptors_for(df_core::TaskKind::Chat)
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(
        !chat
            .iter()
            .any(|n| n.contains("ci_") || n == "list_pull_requests")
    );
}

const WRITABLE_SENTENCE: &str = "Writable: /data/repo (workspace), /data/home (your home), \
         /tmp. Everything else is read-only or inaccessible on purpose; don't try to change \
         permissions.";

#[test]
fn enforced_landlock_names_the_writable_directories_in_the_sandbox_section() {
    let prompt = system_prompt("You are Linus.", &[], LandlockMode::Enforce);
    assert_eq!(
        prompt,
        format!("You are Linus.\n\n## Sandbox\n\n{WRITABLE_SENTENCE}\n")
    );
}

#[test]
fn enforced_landlock_without_a_persona_prompt_still_has_the_section() {
    let prompt = system_prompt("", &[], LandlockMode::Enforce);
    assert_eq!(prompt, format!("## Sandbox\n\n{WRITABLE_SENTENCE}\n"));
}

#[test]
fn disabled_landlock_without_hidden_paths_leaves_the_prompt_alone() {
    assert_eq!(
        system_prompt("You are Linus.", &[], LandlockMode::Disabled),
        "You are Linus."
    );
}

#[test]
fn hidden_paths_and_the_landlock_sentence_share_one_sandbox_block() {
    let hidden = ["talos/**".to_string(), "CODEOWNERS".to_string()];
    let prompt = system_prompt("You are Linus.", &hidden, LandlockMode::Enforce);
    assert_eq!(
        prompt,
        format!(
            "You are Linus.\n\n## Sandbox\n\nThese repository paths are hidden from your \
                 workspace on purpose: talos/**, CODEOWNERS. Do not restore, check out or read \
                 them through Git (`git show`, `git checkout`, `git sparse-checkout`); pushes \
                 changing them are rejected.\n\n{WRITABLE_SENTENCE}\n"
        )
    );
    assert_eq!(prompt.matches("## Sandbox").count(), 1);
}

#[test]
fn hidden_paths_alone_keep_their_section_when_landlock_is_disabled() {
    let hidden = ["talos/**".to_string()];
    let prompt = system_prompt("", &hidden, LandlockMode::Disabled);
    assert!(prompt.starts_with("## Sandbox\n\nThese repository paths are hidden"));
    assert!(!prompt.contains("Writable:"));
}

#[tokio::test]
async fn assign_for_the_first_run_of_df_1() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    let workspace = assign.workspace.expect("workspace");
    assert_eq!(workspace.task_branch, "factory/task/DF-1");
    assert_eq!(workspace.wip_branch, "factory/wip/DF-1");
    assert_eq!(workspace.base_branch, "main");
    assert_eq!(workspace.rewrite_policy(), RewritePolicyProto::WithLease);
    assert!(!workspace.existing_branch);
    assert_eq!(assign.tools.len(), 19);
    assert_eq!(assign.task.expect("task").task_key, "DF-1");
    assert!(assign.continuation.is_none());
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
        anyhow::bail!("expected ACP harness");
    };
    assert_eq!(acp.command, CLAUDE_ADAPTER);
    assert_eq!(
        acp.command,
        env.ctx.config.harness.claude_code.adapter_command
    );
    assert_eq!(
        assign
            .budgets
            .expect("budgets")
            .max_wall_clock
            .expect("limit")
            .seconds,
        7200
    );
    Ok(())
}

#[tokio::test]
async fn assign_carries_the_current_public_signing_key_of_the_persona() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let public = df_db::persona_keys::sync_signing_key(
        &pool,
        env.ctx.secrets.as_ref(),
        env.persona_id.as_uuid(),
    )
    .await?
    .public_key;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let rc = load_run_context(&pool, get_run(&pool, run_id).await?.expect("run")).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    assert_eq!(
        assign.workspace.expect("workspace").signing_public_key,
        public
    );
    Ok(())
}

#[tokio::test]
async fn assign_holds_the_public_signing_key_and_no_private_key_material() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let public = df_db::persona_keys::sync_signing_key(
        &pool,
        env.ctx.secrets.as_ref(),
        env.persona_id.as_uuid(),
    )
    .await?
    .public_key;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let rc = load_run_context(&pool, get_run(&pool, run_id).await?.expect("run")).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    let shown = format!("{assign:?}");
    assert!(shown.contains(&public), "the public key is sent: {shown}");
    assert!(!shown.contains("PRIVATE KEY"), "{shown}");
    Ok(())
}

#[tokio::test]
async fn assign_carries_the_persona_identity_the_run_started_with() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run1 = env.insert_run(env.task.id, 1, "running").await;
    let rc1 = load_run_context(&pool, get_run(&pool, run1).await?.expect("run")).await?;
    let first = build_assign(&env.ctx, &rc1).await?;

    sqlx::query("UPDATE personas SET commit_email = 'linus@corp.example'")
        .execute(&pool)
        .await?;
    // The edit happens while run 1 is active: its Assign (also a rebuilt one) keeps the identity.
    let again = build_assign(&env.ctx, &rc1).await?;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(run1.as_uuid())
        .execute(&pool)
        .await?;
    let run2 = env.insert_run(env.task.id, 2, "connecting").await;
    let rc2 = load_run_context(&pool, get_run(&pool, run2).await?.expect("run")).await?;
    let second = build_assign(&env.ctx, &rc2).await?;

    let identity = |assign: Assign| {
        let identity = assign.workspace.expect("workspace").commit_identity;
        let identity = identity.expect("identity");
        (identity.name, identity.email)
    };
    let linus = ("Linus".to_owned(), "linus@example.org".to_owned());
    assert_eq!(identity(first), linus);
    assert_eq!(identity(again), linus);
    assert_eq!(
        identity(second),
        ("Linus".to_owned(), "linus@corp.example".to_owned())
    );
    Ok(())
}

/// The system prompt file of the harness in `assign`.
fn prompt_file(assign: &Assign, env: &Env) -> String {
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.clone().and_then(|h| h.kind) else {
        panic!("expected ACP harness");
    };
    let path = harness::profile("claude-code").unwrap().system_prompt_path;
    acp.files
        .into_iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no prompt file in {:?}", env.ctx.config.landlock))
        .content
}

// Scenario "Assign for DF-12-CHAT" of `run-orchestration` and "System prompt of a chat run" and
// "Task runs unchanged" of `chats`.
#[tokio::test]
async fn assign_for_a_chat_run_is_read_only_without_branches_and_offers_the_chat_tools()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let chat = env.chat().await;
    let run_id = env.insert_run(chat.id, 1, "connecting").await;
    let rc = load_run_context(&pool, get_run(&pool, run_id).await?.expect("run")).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    let workspace = assign.workspace.clone().expect("workspace");
    assert_eq!(workspace.base_branch, "main");
    assert_eq!(workspace.task_branch, "");
    assert_eq!(workspace.wip_branch, "");
    assert!(workspace.read_only);
    assert!(!workspace.existing_branch);
    assert_eq!(
        assign.task.clone().expect("task").task_key,
        chat.key.to_string()
    );
    let tools: Vec<_> = assign.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tools.contains(&"create_task"), "{tools:?}");
    assert!(tools.contains(&"list_issues"), "{tools:?}");
    assert!(tools.contains(&"create_issue"), "{tools:?}");
    for excluded in [
        "complete_task",
        "ask_question",
        "update_pull_request",
        "comment_on_issue",
    ] {
        assert!(!tools.contains(&excluded), "{excluded} in {tools:?}");
    }
    let prompt = prompt_file(&assign, &env);
    assert!(prompt.starts_with("You are Linus"), "{prompt}");
    assert!(prompt.contains(CHAT_INSTRUCTIONS), "{prompt}");
    Ok(())
}

#[tokio::test]
async fn assign_for_a_task_run_has_no_chat_instructions_and_a_writable_workspace()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let rc = load_run_context(&pool, get_run(&pool, run_id).await?.expect("run")).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    assert!(!assign.workspace.clone().expect("workspace").read_only);
    assert!(!prompt_file(&assign, &env).contains(CHAT_INSTRUCTIONS));
    assert!(assign.tools.iter().any(|t| t.name == "complete_task"));
    for tool in CHAT_ONLY_TOOLS {
        assert!(
            assign.tools.iter().all(|t| t.name != *tool),
            "{tool} is offered to a task"
        );
    }
    Ok(())
}

#[test]
fn the_issue_tools_are_chat_only_and_say_what_a_chat_agent_must_know() {
    assert_eq!(CHAT_ONLY_TOOLS, ["list_issues", "create_issue"]);
    for tool in CHAT_ONLY_TOOLS {
        assert!(CHAT_TOOLS.contains(tool), "{tool}");
    }
    let tools = tool_descriptors();
    let find = |name: &str| tools.iter().find(|t| t.name == name).unwrap();
    assert!(find("list_issues").description.contains("untrusted input"));
    let create = &find("create_issue").description;
    assert!(
        create.contains("Nothing is posted before a member approves"),
        "{create}"
    );
    assert!(
        create.contains("no secrets and no workspace data"),
        "{create}"
    );
    assert!(create.contains("create_task with issue"), "{create}");
    assert!(find("create_task").input_schema_json.contains("\"issue\""));
}

#[test]
fn the_chat_instructions_say_what_a_chat_agent_must_know() {
    for part in [
        "board member",
        "read-only",
        "create_task",
        "send_message",
        "never completed",
    ] {
        assert!(CHAT_INSTRUCTIONS.contains(part), "{part}");
    }
}

#[test]
fn every_chat_tool_is_a_factory_tool() {
    let names: Vec<_> = tool_descriptors().into_iter().map(|t| t.name).collect();
    for tool in CHAT_TOOLS {
        assert!(names.iter().any(|name| name == tool), "{tool}");
    }
}

// Scenarios "Based on a task" and "Base task merged" of `task-tree`.
#[tokio::test]
async fn the_workspace_base_branch_is_the_base_tasks_branch_until_that_task_is_done()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let reviewer = env.task_based_on(&env.task).await;
    let run_id = env.insert_run(reviewer.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");

    let rc = load_run_context(&pool, run.clone()).await?;
    let workspace = build_assign(&env.ctx, &rc)
        .await?
        .workspace
        .expect("workspace");
    assert_eq!(workspace.base_branch, "factory/task/DF-1");
    assert_eq!(workspace.task_branch, "factory/task/DF-2");
    // The Git proxy's route names the same base.
    let route_base = || async {
        let routes = df_db::egress::active_runs(&pool).await?;
        let route = routes.into_iter().find(|r| r.task_key == "DF-2");
        anyhow::Ok(route.expect("route of DF-2").base_branch)
    };
    assert_eq!(route_base().await?, "factory/task/DF-1");

    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await?;
    let rc = load_run_context(&pool, run).await?;
    let workspace = build_assign(&env.ctx, &rc)
        .await?
        .workspace
        .expect("workspace");
    assert_eq!(workspace.base_branch, "main");
    assert_eq!(route_base().await?, "main");
    Ok(())
}

#[test]
fn proxy_url_names_the_task_on_the_gateway_and_omits_port_443() {
    let proxy = |port| GitProxy {
        host: "df-gateway.df.svc".into(),
        port,
    };
    assert_eq!(
        proxy_repo_url(&proxy(8443), "DF-7"),
        "https://df-gateway.df.svc:8443/git/DF-7.git"
    );
    assert_eq!(
        proxy_repo_url(&proxy(443), "DF-7"),
        "https://df-gateway.df.svc/git/DF-7.git"
    );
}

#[tokio::test]
async fn assign_carries_the_repository_through_the_proxy() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.config = std::sync::Arc::new(ChannelConfig {
        git_proxy: GitProxy {
            host: "df-gateway.df.svc".into(),
            port: 8443,
        },
        ..ChannelConfig::default()
    });
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    let workspace = assign.workspace.expect("workspace");
    assert_eq!(
        workspace.repo_url,
        "https://df-gateway.df.svc:8443/git/DF-1.git"
    );
    assert_eq!(workspace.task_branch, "factory/task/DF-1");
    Ok(())
}

#[tokio::test]
async fn assign_uses_the_project_adapter_command_when_set() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("UPDATE projects SET adapter_command = ARRAY['/opt/harness/bin/fake-acp-agent']")
        .execute(&pool)
        .await?;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
        anyhow::bail!("expected ACP harness");
    };
    assert_eq!(acp.command, ["/opt/harness/bin/fake-acp-agent"]);
    Ok(())
}

#[tokio::test]
async fn a_project_with_hidden_paths_gets_them_and_the_sandbox_prompt_section() -> anyhow::Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query(
        "INSERT INTO project_push_policies
                 (project_id, protected_globs, protected_mode, hidden_globs, secret_mode,
                  size_mode, max_blob_bytes)
             SELECT id, '{}', 'approval', ARRAY['talos/**', 'CODEOWNERS'], 'approval',
                    'approval', 1024
             FROM projects",
    )
    .execute(&pool)
    .await?;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    assert_eq!(
        assign.workspace.expect("workspace").hidden_paths,
        ["talos/**", "CODEOWNERS"]
    );
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
        anyhow::bail!("expected ACP harness");
    };
    let prompt = &acp.files[0].content;
    assert!(prompt.starts_with("You are Linus."), "{prompt}");
    assert!(
        prompt.contains(
            "\n\n## Sandbox\n\nThese repository paths are hidden from your workspace on \
                 purpose: talos/**, CODEOWNERS. Do not restore, check out or read them through \
                 Git (`git show`, `git checkout`, `git sparse-checkout`); pushes changing them \
                 are rejected.\n\n"
        ),
        "{prompt}"
    );
    assert!(
        prompt.ends_with(&format!("\n\n{WRITABLE_SENTENCE}\n")),
        "{prompt}"
    );
    assert_eq!(prompt.matches("## Sandbox").count(), 1, "{prompt}");
    Ok(())
}

#[tokio::test]
async fn a_policy_change_between_runs_reaches_the_next_run_only() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run1 = env.insert_run(env.task.id, 1, "running").await;
    let rc1 = load_run_context(&pool, get_run(&pool, run1).await?.expect("run")).await?;
    let first = build_assign(&env.ctx, &rc1).await?;

    sqlx::query(
        "INSERT INTO project_push_policies
                 (project_id, protected_globs, protected_mode, hidden_globs, secret_mode,
                  size_mode, max_blob_bytes)
             SELECT id, '{}', 'approval', ARRAY['talos/**'], 'approval', 'approval', 1024
             FROM projects",
    )
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(run1.as_uuid())
        .execute(&pool)
        .await?;
    let run2 = env.insert_run(env.task.id, 2, "connecting").await;
    let rc2 = load_run_context(&pool, get_run(&pool, run2).await?.expect("run")).await?;
    let second = build_assign(&env.ctx, &rc2).await?;

    assert!(first.workspace.expect("workspace").hidden_paths.is_empty());
    assert_eq!(
        second.workspace.expect("workspace").hidden_paths,
        ["talos/**"]
    );
    Ok(())
}

#[tokio::test]
async fn a_project_without_hidden_paths_gets_only_the_landlock_sentence_in_the_section()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&pool, run_id).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let assign = build_assign(&env.ctx, &rc).await?;

    assert!(assign.workspace.expect("workspace").hidden_paths.is_empty());
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
        anyhow::bail!("expected ACP harness");
    };
    assert_eq!(
        acp.files[0].content,
        format!("You are Linus.\n\n## Sandbox\n\n{WRITABLE_SENTENCE}\n")
    );
    Ok(())
}

#[tokio::test]
async fn disabled_landlock_leaves_the_system_prompt_without_a_sandbox_section() -> anyhow::Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    env.ctx.config = std::sync::Arc::new(ChannelConfig {
        landlock: LandlockMode::Disabled,
        ..ChannelConfig::default()
    });

    let (_, files) = assign_of(&env).await?;

    assert_eq!(files[0].1, "You are Linus.");
    Ok(())
}

#[tokio::test]
async fn follow_up_run_carries_session_id_and_summary() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run1 = env.insert_run(env.task.id, 1, "running").await;
    sqlx::query("UPDATE runs SET state = 'succeeded', harness_session_id = 'S' WHERE id = $1")
        .bind(run1.as_uuid())
        .execute(&pool)
        .await?;
    let run2 = env.insert_run(env.task.id, 2, "connecting").await;
    let run = get_run(&pool, run2).await?.expect("run");
    let rc = load_run_context(&pool, run).await?;

    let continuation = build_assign(&env.ctx, &rc)
        .await?
        .continuation
        .expect("continuation");
    assert_eq!(continuation.harness_session_id, "S");
    assert!(continuation.summary.contains("Fix login"));
    Ok(())
}

/// The environment of the harness in the `Assign` of a run of `Env`'s persona.
async fn harness_env(env: &Env) -> anyhow::Result<std::collections::HashMap<String, String>> {
    harness_env_of_run(env, 1).await
}

/// The environment of the harness in the `Assign` of run `number` of `Env`'s task. Marks the
/// earlier runs `failed` first, since only one run of a task may be active.
async fn harness_env_of_run(
    env: &Env,
    number: i64,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    sqlx::query("UPDATE runs SET state = 'failed' WHERE number < $1")
        .bind(number)
        .execute(&env.ctx.pool)
        .await?;
    let run = env.insert_run(env.task.id, number, "connecting").await;
    let run = get_run(&env.ctx.pool, run).await?.expect("run");
    let rc = load_run_context(&env.ctx.pool, run).await?;
    let assign = build_assign(&env.ctx, &rc).await?;
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.and_then(|h| h.kind) else {
        anyhow::bail!("no ACP harness");
    };
    Ok(acp.env)
}

#[tokio::test]
async fn claude_code_with_a_subscription_gets_the_gateway_url_a_placeholder_and_the_ca()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;

    let vars = harness_env(&env).await?;

    assert_eq!(
        vars["ANTHROPIC_BASE_URL"],
        inference_base_url(&env.ctx.config.git_proxy)
    );
    assert!(vars["ANTHROPIC_BASE_URL"].ends_with("/inference"));
    assert_eq!(vars["CLAUDE_CODE_OAUTH_TOKEN"], "df-placeholder");
    assert_eq!(vars["NODE_EXTRA_CA_CERTS"], "/run/factory/ca/ca.crt");
    assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
    assert_eq!(
        vars["CLAUDE_CODE_EXECUTABLE"],
        "/opt/harness/claude-code/bin/claude"
    );
    assert!(!vars.contains_key("ANTHROPIC_API_KEY"));
    assert!(
        vars.values()
            .all(|value| !value.contains("claude-secret-value")),
        "no model credential in the sandbox environment"
    );
    Ok(())
}

#[tokio::test]
async fn claude_code_gets_the_model_of_the_persona_in_every_setup() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;

    let subscription = harness_env(&env).await?;
    sqlx::query("UPDATE model_providers SET credential_scheme = 'x-api-key'")
        .execute(&pool)
        .await?;
    let api_key = harness_env_of_run(&env, 2).await?;
    sqlx::query("UPDATE projects SET adapter_command = ARRAY['/usr/local/bin/my-adapter']")
        .execute(&pool)
        .await?;
    let own_adapter = harness_env_of_run(&env, 3).await?;

    // each setup took effect
    assert!(subscription.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
    assert!(api_key.contains_key("ANTHROPIC_API_KEY"));
    assert!(api_key.contains_key("CLAUDE_CODE_EXECUTABLE"));
    assert!(!own_adapter.contains_key("CLAUDE_CODE_EXECUTABLE"));
    for vars in [subscription, api_key, own_adapter] {
        assert_eq!(vars["ANTHROPIC_MODEL"], "claude-sonnet-5-5");
        assert!(!vars.contains_key("ANTHROPIC_SMALL_FAST_MODEL"));
    }
    Ok(())
}

#[tokio::test]
async fn claude_code_with_the_adapter_of_the_project_gets_no_path_into_the_harness_image()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("UPDATE projects SET adapter_command = ARRAY['/usr/local/bin/my-adapter']")
        .execute(&pool)
        .await?;

    let vars = harness_env(&env).await?;

    assert!(!vars.contains_key("CLAUDE_CODE_EXECUTABLE"));
    assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
    Ok(())
}

#[tokio::test]
async fn claude_code_with_an_api_key_provider_gets_anthropic_api_key_and_no_oauth_token()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("UPDATE model_providers SET credential_scheme = 'x-api-key'")
        .execute(&pool)
        .await?;

    let vars = harness_env(&env).await?;

    assert_eq!(vars["ANTHROPIC_API_KEY"], "df-placeholder");
    assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
    assert!(!vars.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
    Ok(())
}

#[tokio::test]
async fn the_inference_environment_wins_over_the_configured_harness_environment()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    let mut config = ChannelConfig::default();
    config
        .harness
        .claude_code
        .env
        .insert("ANTHROPIC_BASE_URL".into(), "https://elsewhere.test".into());
    env.ctx.config = std::sync::Arc::new(config);

    let vars = harness_env(&env).await?;

    assert!(
        vars["ANTHROPIC_BASE_URL"].ends_with("/inference"),
        "{vars:?}"
    );
    Ok(())
}

/// Turns the persona of `Env` into an `OpenCode` persona on a Chat Completions model with
/// limits, next to the Anthropic model, at a provider that offers both APIs.
async fn use_opencode(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query("UPDATE model_providers SET credential_scheme = 'bearer'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
             SELECT id, 'openai-chat', 'https://llm.example.test' FROM model_providers",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO model_provider_models
             (provider_id, model_id, wire_api, context_window_tokens, max_output_tokens)
             SELECT id, 'glm-5.3', 'openai-chat', 1000000, 131072 FROM model_providers",
    )
    .execute(pool)
    .await?;
    sqlx::query("UPDATE personas SET harness = 'opencode', model = 'glm-5.3'")
        .execute(pool)
        .await?;
    Ok(())
}

async fn assign_of(env: &Env) -> anyhow::Result<(AcpHarness, Vec<(String, String)>)> {
    let run = env.insert_run(env.task.id, 1, "connecting").await;
    let run = get_run(&env.ctx.pool, run).await?.expect("run");
    let rc = load_run_context(&env.ctx.pool, run).await?;
    let assign = build_assign(&env.ctx, &rc).await?;
    let Some(harness_kind::Kind::Acp(acp)) = assign.harness.and_then(|h| h.kind) else {
        anyhow::bail!("no ACP harness");
    };
    let files = acp
        .files
        .iter()
        .map(|f| (f.path.clone(), f.content.clone()))
        .collect();
    Ok((acp, files))
}

#[tokio::test]
async fn opencode_gets_its_prompt_file_the_configuration_document_and_no_claude_file()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.config = std::sync::Arc::new(ChannelConfig {
        landlock: LandlockMode::Disabled,
        ..ChannelConfig::default()
    });
    use_opencode(&pool).await?;

    let (acp, files) = assign_of(&env).await?;

    assert_eq!(
        files,
        [(
            ".config/opencode/AGENTS.md".to_string(),
            "You are Linus.".to_string()
        )]
    );
    assert_eq!(acp.command, ["/opt/harness/opencode/bin/opencode", "acp"]);
    assert_eq!(acp.env["OPENCODE_DISABLE_MODELS_FETCH"], "1");
    assert_eq!(acp.env["OPENCODE_DISABLE_AUTOUPDATE"], "1");
    assert_eq!(acp.env["NODE_EXTRA_CA_CERTS"], "/run/factory/ca/ca.crt");
    assert!(!acp.env.contains_key("ANTHROPIC_BASE_URL"));
    assert!(!acp.env.contains_key("ANTHROPIC_API_KEY"));
    assert!(!acp.env.contains_key("ANTHROPIC_MODEL"));
    let document: serde_json::Value = serde_json::from_str(&acp.env["OPENCODE_CONFIG_CONTENT"])?;
    let provider = &document["provider"]["df"];
    assert_eq!(provider["npm"], "@ai-sdk/openai-compatible");
    assert_eq!(
        provider["options"]["baseURL"],
        format!("{}/v1", inference_base_url(&env.ctx.config.git_proxy))
    );
    assert_eq!(provider["options"]["apiKey"], "df-placeholder");
    assert_eq!(
        provider["models"]["glm-5.3"]["limit"],
        serde_json::json!({ "context": 1_000_000, "output": 131_072 })
    );
    assert_eq!(document["model"], "df/glm-5.3");
    assert_eq!(document["enabled_providers"], serde_json::json!(["df"]));
    Ok(())
}

#[tokio::test]
async fn the_run_uses_the_wire_api_of_the_model_not_a_harness_preference() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    use_opencode(&pool).await?;
    sqlx::query(
        "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
             SELECT id, 'openai-responses', 'https://llm.example.test' FROM model_providers",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO model_provider_models (provider_id, model_id, wire_api)
             SELECT id, 'gpt-6-luna', 'openai-responses' FROM model_providers",
    )
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE personas SET model = 'gpt-6-luna'")
        .execute(&pool)
        .await?;

    let (acp, _) = assign_of(&env).await?;

    let document: serde_json::Value = serde_json::from_str(&acp.env["OPENCODE_CONFIG_CONTENT"])?;
    assert_eq!(document["provider"]["df"]["npm"], "@ai-sdk/openai");
    assert!(document["provider"]["df"]["models"]["gpt-6-luna"]["limit"].is_null());
    Ok(())
}

#[tokio::test]
async fn claude_code_gets_claude_files_and_the_configured_settings() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    let mut config = ChannelConfig::default();
    config.harness.claude_code.settings_json = r#"{"theme":"dark"}"#.into();
    config.landlock = LandlockMode::Disabled;
    env.ctx.config = std::sync::Arc::new(config);

    let (acp, files) = assign_of(&env).await?;

    assert_eq!(
        files,
        [
            (
                ".claude/CLAUDE.md".to_string(),
                "You are Linus.".to_string()
            ),
            (
                ".claude/settings.json".to_string(),
                r#"{"theme":"dark"}"#.to_string()
            )
        ]
    );
    assert!(!acp.env.contains_key("OPENCODE_CONFIG_CONTENT"));
    Ok(())
}

#[tokio::test]
async fn claude_code_without_settings_gets_only_the_prompt_file() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;

    let (_, files) = assign_of(&env).await?;

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, ".claude/CLAUDE.md");
    Ok(())
}

#[tokio::test]
async fn a_model_the_harness_cannot_use_fails_the_assignment() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    use_opencode(&pool).await?;
    sqlx::query("UPDATE personas SET harness = 'claude-code'")
        .execute(&pool)
        .await?;

    assert!(assign_of(&env).await.is_err());
    Ok(())
}

#[test]
fn principals_convert_to_the_wire_type() {
    let p = principal(&Principal::system(df_core::system_reason::TASK_ASSIGNED));
    assert!(
        matches!(p.kind, Some(transcript::principal::Kind::System(s)) if s.reason == df_core::system_reason::TASK_ASSIGNED)
    );
}

#[tokio::test]
async fn assign_of_a_task_on_an_existing_branch_carries_it_the_pr_base_and_the_policy()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("UPDATE projects SET branch_rewrite_policy = 'never'")
        .execute(&pool)
        .await?;
    let task = env
        .task_on_branch(
            "fix/cve",
            Some(df_db::AttachedPr {
                number: 41,
                url: "https://git.example.org/pulls/41".into(),
                base: "release/1.2".into(),
            }),
        )
        .await;
    let run_id = env.insert_run(task.id, 1, "connecting").await;
    let rc = load_run_context(&pool, get_run(&pool, run_id).await?.expect("run")).await?;

    let workspace = build_assign(&env.ctx, &rc)
        .await?
        .workspace
        .expect("workspace");

    assert_eq!(workspace.task_branch, "fix/cve");
    assert_eq!(workspace.wip_branch, format!("factory/wip/{}", task.key));
    assert_eq!(workspace.base_branch, "release/1.2");
    assert!(workspace.existing_branch);
    assert_eq!(workspace.rewrite_policy(), RewritePolicyProto::Never);
    Ok(())
}

/// A resumed run of a task whose prompts were all delivered starts with the resume prompt of
/// the system, so the agent has a turn to continue in.
#[tokio::test]
async fn a_resumed_run_is_prompted_with_the_system_resume_prompt() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    sqlx::query("DELETE FROM task_messages WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await?;
    let mut tx = pool.begin().await?;
    assert!(df_db::tasks::queue_resume_prompt_tx(&mut tx, env.task.id).await?);
    tx.commit().await?;

    let prompts = pending_prompts(&pool, env.task.id, false).await?;

    let [prompt] = prompts.as_slice() else {
        anyhow::bail!("expected one prompt, got {}", prompts.len());
    };
    let [block] = prompt.content.as_slice() else {
        anyhow::bail!("expected one content block");
    };
    assert_eq!(
        block.kind,
        Some(df_proto::transcript::v1::content_block::Kind::Text(
            df_proto::transcript::v1::TextContent {
                text: df_db::tasks::RESUME_PROMPT.into(),
            }
        ))
    );
    assert_eq!(
        prompt.author,
        Some(principal(&Principal::system(
            df_core::system_reason::RUN_RESUMED
        )))
    );
    Ok(())
}
