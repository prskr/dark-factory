//! The delegation tools: `create_task`, `list_personas` and `send_message` (spec
//! `factory-tools`, design D5).

use df_core::{
    DependencyKind, ExistingBranch, Principal, TaskKey, matches_agent_branch, sanitize_title,
};
use df_db::tasks::child_task_id;
use df_db::{DbError, NewDependency, NewTask, TaskPlacement, catalog, get_task, place_on_branch};
use serde_json::{Value, json};

use super::{Outcome, text_arg};
use crate::ChannelContext;
use crate::error::ChannelError;
use df_orchestrator::OrchestratorError;
use df_orchestrator::run_context::RunContext;

/// Creates a task in the run's project with the run's task as parent and the run's agent as
/// creator.
pub(super) async fn create_task(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let persona_name = text_arg(args, "persona").unwrap_or_default();
    let persona = catalog::persona_id_by_name(&ctx.pool, &persona_name).await?;
    let Some(persona) = persona else {
        return Ok(Outcome::error(format!(
            "unknown persona `{persona_name}`; choose one from list_personas"
        )));
    };
    let Some(title) = text_arg(args, "title").as_deref().and_then(sanitize_title) else {
        return Ok(Outcome::error("the title is empty"));
    };
    let prompt = text_arg(args, "prompt").unwrap_or_default();
    if prompt.trim().is_empty() {
        return Ok(Outcome::error("the prompt is empty"));
    }
    let depends_on = match parse_dependencies(args) {
        Ok(depends_on) => depends_on,
        Err(problem) => return Ok(Outcome::error(problem)),
    };
    let placement = match resolve_branch(ctx, rc, args).await {
        Ok(placement) => placement,
        Err(problem) => return Ok(Outcome::error(problem)),
    };
    let input = NewTask {
        project_id: rc.project.id,
        persona_id: df_core::PersonaId::from_uuid(persona),
        title: Some(title),
        description: prompt,
        issue_number: None,
        created_by: agent(rc),
        parent_id: Some(rc.task.id),
        depends_on,
    };
    match df_db::create_task_on(&ctx.pool, input, placement).await {
        Ok((task, _)) => {
            let mut outcome = Outcome::ok(format!(
                "Created {} with status {}. End your turn: you are woken when it ends.",
                task.key, task.status
            ));
            outcome.structured = Some(json!({ "key": task.key, "status": task.status }));
            Ok(outcome)
        }
        Err(DbError::InvalidDependency(key)) => Ok(Outcome::error(format!(
            "invalid dependency `{key}`: it is not a task of this project, or it is listed twice"
        ))),
        Err(held @ DbError::BranchHeld { .. }) => Ok(Outcome::error(held.to_string())),
        Err(e) => Err(e.into()),
    }
}

/// The placement of the `branch` argument: validated, matched against the project's agent
/// branches and attached to its one open PR. The `Err` is what the agent is told.
async fn resolve_branch(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<TaskPlacement, String> {
    let Some(branch) = text_arg(args, "branch") else {
        return Ok(TaskPlacement::default());
    };
    if args.get("base_task").is_some() {
        return Err(
            "`branch` can't be combined with `base_task`: a task on an existing branch has no base task"
                .into(),
        );
    }
    let branch = ExistingBranch::parse(&branch, &rc.project.base_branch)
        .map_err(|problem| format!("invalid branch: {problem}"))?;
    let globs = &rc.project.agent_branch_globs;
    if !matches_agent_branch(globs, branch.as_str()) {
        let allowed = if globs.is_empty() {
            "none".to_owned()
        } else {
            globs.join(", ")
        };
        return Err(format!(
            "`{branch}` is not one of the agent branches of project {} (allowed: {allowed}); ask a board member to start a task on it",
            rc.project.key
        ));
    }
    place_on_branch(&*ctx.forge, &rc.project.id, branch)
        .await
        .map_err(|problem| problem.to_string())
}

/// Every persona with its description, ordered by name.
pub(super) async fn list_personas(ctx: &ChannelContext) -> Result<Outcome, ChannelError> {
    let rows = catalog::list_personas(&ctx.pool).await?;
    let text = rows
        .iter()
        .map(|persona| {
            if persona.description.is_empty() {
                format!("- {}", persona.name)
            } else {
                format!("- {}: {}", persona.name, persona.description)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let personas: Vec<Value> = rows
        .iter()
        .map(|persona| json!({ "name": persona.name, "description": persona.description }))
        .collect();
    let mut outcome = Outcome::ok(text);
    outcome.structured = Some(json!({ "personas": personas }));
    Ok(outcome)
}

/// Queues a message on a child of the run's task, with the agent as author.
pub(super) async fn send_message(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let key = text_arg(args, "task").unwrap_or_default();
    let text = text_arg(args, "text").unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(Outcome::error("the text is empty"));
    }
    let child = match key.parse::<TaskKey>() {
        Ok(key) => child_task_id(&ctx.pool, rc.project.id, &key, rc.task.id).await?,
        Err(_invalid) => None,
    };
    let Some(child) = child else {
        return Ok(Outcome::error(format!(
            "`{key}` is not a child of this task; you can message only tasks you created"
        )));
    };
    let content = vec![df_db::text_block(text, false)];
    let mut tx = ctx.pool.begin().await?;
    let message = match df_db::queue_message_tx(&mut tx, child, agent(rc), content).await {
        Ok(message) => message,
        Err(held @ DbError::BranchHeld { .. }) => return Ok(Outcome::error(held.to_string())),
        Err(e) => return Err(e.into()),
    };
    let status = get_task(&mut *tx, child)
        .await?
        .ok_or_else(|| OrchestratorError::InvalidData(format!("task {child} vanished")))?
        .status
        .to_string();
    tx.commit().await?;
    let mut outcome = Outcome::ok(format!(
        "Queued as prompt {}; {key} is now {status}. End your turn: you are woken when it ends.",
        message.id
    ));
    outcome.structured = Some(json!({ "prompt_id": message.id, "status": status }));
    Ok(outcome)
}

fn agent(rc: &RunContext) -> Principal {
    Principal::agent(
        rc.persona.id,
        rc.persona.name.clone(),
        Some(rc.run.id),
        rc.task.id,
    )
}

/// The `depends_on` argument; a problem names the entry.
fn parse_dependencies(args: &Value) -> Result<Vec<NewDependency>, String> {
    let Some(entries) = args.get("depends_on").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    entries
        .iter()
        .map(|entry| {
            let task = entry
                .get("task")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let kind = entry
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(NewDependency {
                key: task
                    .parse()
                    .map_err(|_invalid| format!("invalid dependency `{task}`: not a task key"))?,
                kind: kind
                    .parse::<DependencyKind>()
                    .map_err(|e| format!("invalid dependency `{task}`: {e}"))?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
