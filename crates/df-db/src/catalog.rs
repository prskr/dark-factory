//! Configuration records edited in the UI: credential references, projects and personas.
//!
//! Every create or update writes an audit entry with the before/after values in the same
//! transaction. Credential references hold the name of a Kubernetes Secret and a key, never a
//! value.

use df_core::webhooks::WebhookWake;
use df_core::{CredentialRef, CredentialRefId, Principal, SecretKeyRef};
use df_core::{RewritePolicy, SubtaskMerge};
use df_sandbox_contract::glob::protected_glob;
use serde::Serialize;
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::persona_keys::PublicSigningKey;
use crate::pool::DbError;
use crate::push_checks::{PushPolicy, save_push_policy};

/// Who performs a change; recorded in the audit entry.
#[derive(Debug, Clone, Copy)]
pub struct Actor<'a> {
    pub principal: &'a Principal,
    pub roles: &'a [String],
}

/// Fields of a credential reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialRefInput {
    pub name: String,
    pub secret_name: String,
    pub secret_key: String,
}

/// Fields of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectInput {
    pub key: String,
    pub name: String,
    pub repo_url: String,
    pub issue_tracker_url: Option<String>,
    pub image: String,
    pub base_branch: String,
    /// Command that starts the adapter inside the project image; empty means the instance's
    /// harness image and adapter are used.
    pub adapter_command: Vec<String>,
    pub forge_credential_id: Option<Uuid>,
    /// Whose reviews and mentions wake the project's tasks.
    pub webhook_wake: WebhookWake,
    /// Whether agents may rebase their task branch (`never` or `with-lease`).
    pub branch_rewrite_policy: RewritePolicy,
    /// Globs of the existing branches an agent may start tasks on (`create_task`).
    pub agent_branch_globs: Vec<String>,
    /// Who merges the pull requests of subtasks into their parent's branch.
    pub subtask_merge: SubtaskMerge,
}

/// A stored project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectRecord {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub repo_url: String,
    pub issue_tracker_url: Option<String>,
    pub image: String,
    pub base_branch: String,
    pub adapter_command: Vec<String>,
    pub forge_credential_id: Option<Uuid>,
    pub webhook_wake: WebhookWake,
    pub branch_rewrite_policy: RewritePolicy,
    pub agent_branch_globs: Vec<String>,
    pub subtask_merge: SubtaskMerge,
    /// Whether tasks exist.
    pub has_tasks: bool,
}

/// Fields of a persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonaInput {
    /// Unique, lowercase `[a-z0-9-]`; how tools address the persona.
    pub handle: String,
    pub display_name: String,
    /// Author and committer of the persona's commits; unique, lowercase.
    pub commit_email: String,
    /// Role and strengths, at most [`df_core::PERSONA_DESCRIPTION_MAX_CHARS`] characters; agents read it
    /// through `list_personas`.
    pub description: String,
    pub system_prompt: String,
    pub harness: String,
    pub model: String,
    pub model_provider_id: Uuid,
}

/// The unique parts of a persona identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaField {
    Handle,
    CommitEmail,
}

impl std::fmt::Display for PersonaField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Handle => "handle",
            Self::CommitEmail => "commit email",
        })
    }
}

/// A stored persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonaRecord {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub commit_email: String,
    pub description: String,
    pub system_prompt: String,
    pub harness: String,
    pub model: String,
    pub model_provider_id: Uuid,
    /// The public half of the signing key; `None` until the key exists.
    pub signing_key: Option<PublicSigningKey>,
}

/// The columns of a persona row: the signing key is two nullable columns.
struct PersonaRow {
    id: Uuid,
    handle: String,
    display_name: String,
    commit_email: String,
    description: String,
    system_prompt: String,
    harness: String,
    model: String,
    model_provider_id: Uuid,
    signing_public_key: Option<String>,
    signing_fingerprint: Option<String>,
}

impl From<PersonaRow> for PersonaRecord {
    fn from(row: PersonaRow) -> Self {
        let signing_key = match (row.signing_public_key, row.signing_fingerprint) {
            (Some(public_key), Some(fingerprint)) => Some(PublicSigningKey {
                public_key,
                fingerprint,
            }),
            _ => None,
        };
        Self {
            id: row.id,
            handle: row.handle,
            display_name: row.display_name,
            commit_email: row.commit_email,
            description: row.description,
            system_prompt: row.system_prompt,
            harness: row.harness,
            model: row.model,
            model_provider_id: row.model_provider_id,
            signing_key,
        }
    }
}

struct CredentialRow {
    id: Uuid,
    name: String,
    secret_name: String,
    secret_key: String,
}

impl From<CredentialRow> for CredentialRef {
    fn from(r: CredentialRow) -> Self {
        Self {
            id: CredentialRefId::from_uuid(r.id),
            name: r.name,
            secret: SecretKeyRef {
                name: r.secret_name,
                key: r.secret_key,
            },
        }
    }
}

/// A project row as `query_as!` reads it; the enum columns are parsed by [`ProjectRecord::try_from`].
struct ProjectRow {
    id: Uuid,
    key: String,
    name: String,
    repo_url: String,
    issue_tracker_url: Option<String>,
    image: String,
    base_branch: String,
    adapter_command: Vec<String>,
    forge_credential_id: Option<Uuid>,
    webhook_wake: String,
    branch_rewrite_policy: String,
    agent_branch_globs: Vec<String>,
    subtask_merge: String,
    has_tasks: bool,
}

impl TryFrom<ProjectRow> for ProjectRecord {
    type Error = DbError;

    fn try_from(row: ProjectRow) -> Result<Self, DbError> {
        Ok(Self {
            id: row.id,
            key: row.key,
            name: row.name,
            repo_url: row.repo_url,
            issue_tracker_url: row.issue_tracker_url,
            image: row.image,
            base_branch: row.base_branch,
            adapter_command: row.adapter_command,
            forge_credential_id: row.forge_credential_id,
            webhook_wake: row.webhook_wake.parse()?,
            branch_rewrite_policy: row.branch_rewrite_policy.parse()?,
            agent_branch_globs: row.agent_branch_globs,
            subtask_merge: row.subtask_merge.parse()?,
            has_tasks: row.has_tasks,
        })
    }
}

pub async fn list_credential_refs(pool: &PgPool) -> Result<Vec<CredentialRef>, DbError> {
    let rows = sqlx::query_as!(
        CredentialRow,
        "SELECT id, name, secret_name, secret_key FROM credential_refs ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_credential_ref(pool: &PgPool, id: Uuid) -> Result<Option<CredentialRef>, DbError> {
    let row = get_credential_row(pool, id).await?;
    Ok(row.map(Into::into))
}

async fn get_credential_row(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<CredentialRow>, sqlx::Error> {
    sqlx::query_as!(
        CredentialRow,
        "SELECT id, name, secret_name, secret_key FROM credential_refs WHERE id = $1",
        id
    )
    .fetch_optional(executor)
    .await
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectRecord>, DbError> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT p.id, p.key, p.name, p.repo_url, p.issue_tracker_url, p.image, p.base_branch,
               p.adapter_command, p.forge_credential_id, p.webhook_wake, p.branch_rewrite_policy,
               p.agent_branch_globs, p.subtask_merge,
               EXISTS (SELECT 1 FROM tasks t WHERE t.project_id = p.id) AS "has_tasks!"
        FROM projects p ORDER BY p.key
        "#
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ProjectRecord::try_from)
    .collect()
}

pub async fn get_project_by_key(
    pool: &PgPool,
    key: &str,
) -> Result<Option<ProjectRecord>, DbError> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT p.id, p.key, p.name, p.repo_url, p.issue_tracker_url, p.image, p.base_branch,
               p.adapter_command, p.forge_credential_id, p.webhook_wake, p.branch_rewrite_policy,
               p.agent_branch_globs, p.subtask_merge,
               EXISTS (SELECT 1 FROM tasks t WHERE t.project_id = p.id) AS "has_tasks!"
        FROM projects p WHERE p.key = $1
        "#,
        key
    )
    .fetch_optional(pool)
    .await?
    .map(ProjectRecord::try_from)
    .transpose()
}

pub async fn get_project(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<ProjectRecord>, DbError> {
    sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT p.id, p.key, p.name, p.repo_url, p.issue_tracker_url, p.image, p.base_branch,
               p.adapter_command, p.forge_credential_id, p.webhook_wake, p.branch_rewrite_policy,
               p.agent_branch_globs, p.subtask_merge,
               EXISTS (SELECT 1 FROM tasks t WHERE t.project_id = p.id) AS "has_tasks!"
        FROM projects p WHERE p.id = $1
        "#,
        id
    )
    .fetch_optional(executor)
    .await?
    .map(ProjectRecord::try_from)
    .transpose()
}

pub async fn list_personas(pool: &PgPool) -> Result<Vec<PersonaRecord>, DbError> {
    Ok(sqlx::query_as!(
        PersonaRow,
        "SELECT id, handle, display_name, commit_email, description, system_prompt, harness, model,
                model_provider_id, signing_public_key, signing_fingerprint
         FROM personas ORDER BY handle"
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(PersonaRecord::from)
    .collect())
}

pub async fn get_persona(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<PersonaRecord>, DbError> {
    Ok(sqlx::query_as!(
        PersonaRow,
        "SELECT id, handle, display_name, commit_email, description, system_prompt, harness, model,
                model_provider_id, signing_public_key, signing_fingerprint
         FROM personas WHERE id = $1",
        id
    )
    .fetch_optional(executor)
    .await?
    .map(PersonaRecord::from))
}

/// The id of the persona with `handle`.
pub async fn persona_id_by_handle(
    executor: impl PgExecutor<'_>,
    handle: &str,
) -> Result<Option<Uuid>, DbError> {
    Ok(
        sqlx::query_scalar!("SELECT id FROM personas WHERE handle = $1", handle)
            .fetch_optional(executor)
            .await?,
    )
}

/// Creates (`id` is `None`) or updates a credential reference and audits the change.
pub async fn save_credential_ref(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &CredentialRefInput,
) -> Result<CredentialRef, DbError> {
    let mut tx = pool.begin().await?;
    let before: Option<CredentialRow> = match id {
        Some(id) => Some(
            sqlx::query_as!(
                CredentialRow,
                "SELECT id, name, secret_name, secret_key FROM credential_refs WHERE id = $1 FOR UPDATE",
                id
            )
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("credential reference `{id}`")))?,
        ),
        None => None,
    };
    let row: CredentialRow = match &before {
        Some(b) => {
            sqlx::query_as!(
                CredentialRow,
                r#"
                UPDATE credential_refs
                SET name = $2, secret_name = $3, secret_key = $4, updated_at = now()
                WHERE id = $1
                RETURNING id AS "id!", name AS "name!", secret_name AS "secret_name!",
                          secret_key AS "secret_key!"
                "#,
                b.id,
                input.name,
                input.secret_name,
                input.secret_key
            )
            .fetch_one(&mut *tx)
            .await
        }
        None => {
            sqlx::query_as!(
                CredentialRow,
                r#"
                INSERT INTO credential_refs (id, name, secret_name, secret_key)
                VALUES ($1, $2, $3, $4)
                RETURNING id AS "id!", name AS "name!", secret_name AS "secret_name!",
                          secret_key AS "secret_key!"
                "#,
                Uuid::now_v7(),
                input.name,
                input.secret_name,
                input.secret_key
            )
            .fetch_one(&mut *tx)
            .await
        }
    }
    .map_err(|e| conflict_or(e, "a credential reference with this name already exists"))?;

    let before_input = before.map(|b| CredentialRefInput {
        name: b.name,
        secret_name: b.secret_name,
        secret_key: b.secret_key,
    });
    audit(
        &mut tx,
        actor,
        "credential_ref",
        &row.name,
        row.id,
        before_input.as_ref(),
        input,
    )
    .await?;
    tx.commit().await?;
    Ok(row.into())
}

/// Deletes a credential reference nothing uses and audits it. One a project (as forge
/// credential), a model provider or an MCP server uses is a [`DbError::Conflict`] naming the user.
pub async fn delete_credential_ref(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Uuid,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    let (name,): (String,) =
        sqlx::query_as("SELECT name FROM credential_refs WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("credential reference `{id}`")))?;
    let servers = crate::mcp_servers::servers_using_credential(&mut *tx, id).await?;
    let projects: Vec<String> =
        sqlx::query_scalar("SELECT key FROM projects WHERE forge_credential_id = $1 ORDER BY key")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let providers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM model_providers WHERE credential_ref_id = $1 ORDER BY name",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for (what, users) in [
        ("MCP server", servers),
        ("project", projects),
        ("model provider", providers),
    ] {
        if !users.is_empty() {
            return Err(DbError::Conflict(format!(
                "credential `{name}` is used by {what} {}",
                users.join(", ")
            )));
        }
    }
    sqlx::query("DELETE FROM credential_refs WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "credential_ref.delete",
        "credential_ref",
        &name,
    )
    .with_details(&serde_json::json!({ "id": id }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(())
}

/// Creates or updates a project and audits the change. Changing the key of an existing project
/// is a [`DbError::Conflict`].
pub async fn save_project(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &ProjectInput,
) -> Result<ProjectRecord, DbError> {
    let mut tx = pool.begin().await?;
    let record = save_project_in(&mut tx, actor, id, input).await?;
    tx.commit().await?;
    Ok(record)
}

/// Updates project `id` like [`save_project`] and stores its push policy in the same transaction
/// ([`push_checks::save_push_policy`](crate::push_checks)).
pub async fn update_project_with_push_policy(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Uuid,
    input: &ProjectInput,
    push_policy: &PushPolicy,
) -> Result<ProjectRecord, DbError> {
    let mut tx = pool.begin().await?;
    let record = save_project_in(&mut tx, actor, Some(id), input).await?;
    save_push_policy(&mut tx, actor, record.id, &record.key, push_policy).await?;
    tx.commit().await?;
    Ok(record)
}

async fn save_project_in(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &ProjectInput,
) -> Result<ProjectRecord, DbError> {
    let before: Option<ProjectRecord> = match id {
        Some(id) => {
            sqlx::query!("SELECT 1 AS one FROM projects WHERE id = $1 FOR UPDATE", id)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("project `{id}`")))?;
            Some(
                get_project(&mut **tx, id)
                    .await?
                    .ok_or_else(|| DbError::NotFound(format!("project `{id}`")))?,
            )
        }
        None => None,
    };
    for glob in &input.agent_branch_globs {
        protected_glob(glob)
            .map_err(|error| DbError::InvalidData(format!("agent branch `{glob}`: {error}")))?;
    }
    if let Some(credential) = input.forge_credential_id {
        crate::mcp_servers::check_credential_not_used_by_a_server(tx, credential).await?;
    }
    // The key is set at creation and never changes: task keys (DF-1) and branches derive from it.
    if let Some(b) = &before
        && b.key != input.key
    {
        return Err(DbError::Conflict(
            "the key cannot change after creation".into(),
        ));
    }
    let row_id = match &before {
        Some(b) => {
            sqlx::query_scalar!(
                r#"
                UPDATE projects SET key = $2, name = $3, repo_url = $4, issue_tracker_url = $5,
                    image = $6, base_branch = $7, forge_credential_id = $8, adapter_command = $9,
                    webhook_wake = $10, branch_rewrite_policy = $11, agent_branch_globs = $12,
                    subtask_merge = $13, updated_at = now()
                WHERE id = $1 RETURNING id
                "#,
                b.id,
                input.key,
                input.name,
                input.repo_url,
                input.issue_tracker_url,
                input.image,
                input.base_branch,
                input.forge_credential_id,
                &input.adapter_command,
                input.webhook_wake.as_str(),
                input.branch_rewrite_policy.as_str(),
                &input.agent_branch_globs,
                input.subtask_merge.as_str()
            )
            .fetch_one(&mut **tx)
            .await
        }
        None => {
            sqlx::query_scalar!(
                r#"
                INSERT INTO projects (id, key, name, repo_url, issue_tracker_url, image,
                    base_branch, forge_credential_id, adapter_command, webhook_wake,
                    branch_rewrite_policy, agent_branch_globs, subtask_merge)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING id
                "#,
                Uuid::now_v7(),
                input.key,
                input.name,
                input.repo_url,
                input.issue_tracker_url,
                input.image,
                input.base_branch,
                input.forge_credential_id,
                &input.adapter_command,
                input.webhook_wake.as_str(),
                input.branch_rewrite_policy.as_str(),
                &input.agent_branch_globs,
                input.subtask_merge.as_str()
            )
            .fetch_one(&mut **tx)
            .await
        }
    }
    .map_err(|e| conflict_or(e, "a project with this key already exists"))?;

    let record = get_project(&mut **tx, row_id)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("project `{row_id}`")))?;
    let before_input = before.as_ref().map(project_input);
    audit(
        tx,
        actor,
        "project",
        &record.key,
        record.id,
        before_input.as_ref(),
        input,
    )
    .await?;
    Ok(record)
}

/// Creates or updates a persona and audits the change.
pub async fn save_persona(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &PersonaInput,
) -> Result<PersonaRecord, DbError> {
    let input = &validated(input)?;
    check_persona_model(pool, input).await?;
    let mut tx = pool.begin().await?;
    let before: Option<PersonaRecord> = match id {
        Some(id) => Some(
            sqlx::query_as!(
                PersonaRow,
                "SELECT id, handle, display_name, commit_email, description, system_prompt,
                        harness, model, model_provider_id, signing_public_key,
                        signing_fingerprint
                 FROM personas WHERE id = $1 FOR UPDATE",
                id
            )
            .fetch_optional(&mut *tx)
            .await?
            .map(PersonaRecord::from)
            .ok_or_else(|| DbError::NotFound(format!("persona `{id}`")))?,
        ),
        None => None,
    };
    check_persona_identity_free(&mut tx, before.as_ref().map(|b| b.id), input).await?;
    let record_id = match &before {
        Some(b) => {
            sqlx::query_scalar!(
                r#"
                UPDATE personas SET handle = $8, display_name = $2, commit_email = $9,
                    system_prompt = $3, harness = $4, model = $5, model_provider_id = $6,
                    description = $7, updated_at = now()
                WHERE id = $1 RETURNING id
                "#,
                b.id,
                input.display_name,
                input.system_prompt,
                input.harness,
                input.model,
                input.model_provider_id,
                input.description,
                input.handle,
                input.commit_email
            )
            .fetch_one(&mut *tx)
            .await
        }
        None => {
            sqlx::query_scalar!(
                r#"
                INSERT INTO personas (id, display_name, system_prompt, harness, model,
                    model_provider_id, description, handle, commit_email)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id
                "#,
                Uuid::now_v7(),
                input.display_name,
                input.system_prompt,
                input.harness,
                input.model,
                input.model_provider_id,
                input.description,
                input.handle,
                input.commit_email
            )
            .fetch_one(&mut *tx)
            .await
        }
    }
    // The check above names the holder; this catches a concurrent save.
    .map_err(|e| {
        conflict_or(
            e,
            "a persona with this handle or commit email already exists",
        )
    })?;
    let record = get_persona(&mut *tx, record_id)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("persona `{record_id}`")))?;

    let before_input = before.map(|b| PersonaInput {
        handle: b.handle,
        display_name: b.display_name,
        commit_email: b.commit_email,
        description: b.description,
        system_prompt: b.system_prompt,
        harness: b.harness,
        model: b.model,
        model_provider_id: b.model_provider_id,
    });
    audit(
        &mut tx,
        actor,
        "persona",
        &record.handle,
        record.id,
        before_input.as_ref(),
        input,
    )
    .await?;
    tx.commit().await?;
    Ok(record)
}

/// Validates `input` through `PersonaIdentity::new` and the description limit; returns it with the
/// normalized identity (trimmed name, lower-case email).
fn validated(input: &PersonaInput) -> Result<PersonaInput, DbError> {
    if input.description.chars().count() > df_core::PERSONA_DESCRIPTION_MAX_CHARS {
        return Err(DbError::InvalidData(format!(
            "the description is longer than {} characters",
            df_core::PERSONA_DESCRIPTION_MAX_CHARS
        )));
    }
    let identity =
        df_core::PersonaIdentity::new(&input.handle, &input.display_name, &input.commit_email)
            .map_err(DbError::InvalidPersonaIdentity)?;
    Ok(PersonaInput {
        handle: identity.handle,
        display_name: identity.display_name,
        commit_email: identity.commit_email,
        ..input.clone()
    })
}

/// Fails with the holder's handle when another persona uses the handle or the commit email.
async fn check_persona_identity_free(
    tx: &mut Transaction<'_, Postgres>,
    own_id: Option<Uuid>,
    input: &PersonaInput,
) -> Result<(), DbError> {
    let taken = sqlx::query!(
        r#"
        SELECT handle, (handle = $2) AS "same_handle!"
        FROM personas
        WHERE (handle = $2 OR commit_email = $3) AND id IS DISTINCT FROM $1
        ORDER BY (handle = $2) DESC
        LIMIT 1
        "#,
        own_id,
        input.handle,
        input.commit_email
    )
    .fetch_optional(&mut **tx)
    .await?;
    match taken {
        Some(row) => Err(DbError::PersonaFieldTaken {
            field: if row.same_handle {
                PersonaField::Handle
            } else {
                PersonaField::CommitEmail
            },
            holder: row.handle,
        }),
        None => Ok(()),
    }
}

/// The persona's model must be a model of its provider that the harness can use.
async fn check_persona_model(pool: &PgPool, input: &PersonaInput) -> Result<(), DbError> {
    let provider = crate::model_providers::get_model_provider(pool, input.model_provider_id)
        .await?
        .ok_or_else(|| {
            DbError::NotFound(format!("model provider `{}`", input.model_provider_id))
        })?;
    let model = provider.model(&input.model).ok_or_else(|| {
        DbError::ModelConflict(format!(
            "{} is not a model of provider {}.",
            input.model, provider.input.name
        ))
    })?;
    df_core::harness::check_compatible(&input.harness, model.wire_api, provider.input.scheme)
        .map_err(|e| DbError::ModelConflict(e.to_string()))
}

fn project_input(p: &ProjectRecord) -> ProjectInput {
    ProjectInput {
        key: p.key.clone(),
        name: p.name.clone(),
        repo_url: p.repo_url.clone(),
        issue_tracker_url: p.issue_tracker_url.clone(),
        image: p.image.clone(),
        base_branch: p.base_branch.clone(),
        adapter_command: p.adapter_command.clone(),
        forge_credential_id: p.forge_credential_id,
        webhook_wake: p.webhook_wake,
        branch_rewrite_policy: p.branch_rewrite_policy,
        agent_branch_globs: p.agent_branch_globs.clone(),
        subtask_merge: p.subtask_merge,
    }
}

/// Maps a unique-constraint violation to [`DbError::Conflict`].
fn conflict_or(err: sqlx::Error, message: &str) -> DbError {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => DbError::Conflict(message.into()),
        _ => DbError::Sqlx(err),
    }
}

/// Writes `<resource>.create` or `<resource>.update` with `{before, after}` details.
pub(crate) async fn audit<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    actor: Actor<'_>,
    resource: &str,
    label: &str,
    id: Uuid,
    before: Option<&T>,
    after: &T,
) -> Result<(), DbError> {
    let action = format!(
        "{resource}.{}",
        if before.is_some() { "update" } else { "create" }
    );
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        action,
        resource,
        label,
    )
    .with_details(&serde_json::json!({ "id": id, "before": before, "after": after }))?;
    write_audit_entry(&mut **tx, &entry).await?;
    Ok(())
}

#[cfg(test)]
mod identity_migration_tests;

#[cfg(test)]
mod tests;
