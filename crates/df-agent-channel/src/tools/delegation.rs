//! The delegation tools: `create_task`, `list_personas` and `send_message` (spec
//! `factory-tools`, design D5).

use df_core::{DependencyKind, Principal, TaskKey, sanitize_title};
use df_db::{DbError, NewDependency, NewTask};
use serde_json::{Value, json};

use super::{Outcome, text_arg};
use crate::ChannelContext;
use crate::assign::RunContext;
use crate::error::ChannelError;

/// Creates a task in the run's project with the run's task as parent and the run's agent as
/// creator.
pub(super) async fn create_task(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let persona_name = text_arg(args, "persona").unwrap_or_default();
    let persona: Option<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM personas WHERE name = $1")
        .bind(&persona_name)
        .fetch_optional(&ctx.pool)
        .await?;
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
    match df_db::create_task(&ctx.pool, input).await {
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
        Err(e) => Err(e.into()),
    }
}

/// Every persona with its description, ordered by name.
pub(super) async fn list_personas(ctx: &ChannelContext) -> Result<Outcome, ChannelError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT name, description FROM personas ORDER BY name")
            .fetch_all(&ctx.pool)
            .await?;
    let text = rows
        .iter()
        .map(|(name, description)| {
            if description.is_empty() {
                format!("- {name}")
            } else {
                format!("- {name}: {description}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let personas: Vec<Value> = rows
        .iter()
        .map(|(name, description)| json!({ "name": name, "description": description }))
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
    let child: Option<uuid::Uuid> = match key.parse::<TaskKey>() {
        Ok(key) => {
            sqlx::query_scalar(
                "SELECT id FROM tasks WHERE project_id = $1 AND key = $2 AND parent_id = $3",
            )
            .bind(rc.project.id.as_uuid())
            .bind(key.to_string())
            .bind(rc.task.id.as_uuid())
            .fetch_optional(&ctx.pool)
            .await?
        }
        Err(_invalid) => None,
    };
    let Some(child) = child.map(df_core::TaskId::from_uuid) else {
        return Ok(Outcome::error(format!(
            "`{key}` is not a child of this task; you can message only tasks you created"
        )));
    };
    let content = vec![df_db::text_block(text, false)];
    let mut tx = ctx.pool.begin().await?;
    let message = df_db::queue_message_tx(&mut tx, child, agent(rc), content).await?;
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(child.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
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
