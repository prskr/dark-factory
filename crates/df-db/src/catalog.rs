//! Configuration records edited in the UI: credential references, projects and personas.
//!
//! Every create or update writes an audit entry with the before/after values in the same
//! transaction. Credential references hold the name of a Kubernetes Secret and a key, never a
//! value.

use df_core::{CredentialRef, CredentialRefId, Principal};
use serde::Serialize;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::pool::DbError;

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
    pub forge_credential_id: Option<Uuid>,
}

/// A stored project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct ProjectRecord {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub repo_url: String,
    pub issue_tracker_url: Option<String>,
    pub image: String,
    pub base_branch: String,
    pub forge_credential_id: Option<Uuid>,
    /// Whether tasks exist; the key is immutable then.
    pub has_tasks: bool,
}

/// Fields of a persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonaInput {
    pub name: String,
    pub system_prompt: String,
    pub harness: String,
    pub adapter_command: Vec<String>,
    pub model: Option<String>,
    pub harness_credential_id: Option<Uuid>,
}

/// A stored persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct PersonaRecord {
    pub id: Uuid,
    pub name: String,
    pub system_prompt: String,
    pub harness: String,
    pub adapter_command: Vec<String>,
    pub model: Option<String>,
    pub harness_credential_id: Option<Uuid>,
}

#[derive(FromRow)]
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
            secret_name: r.secret_name,
            secret_key: r.secret_key,
        }
    }
}

/// `SELECT` of a project with a trailing clause (`WHERE ...`/`ORDER BY ...`).
macro_rules! select_project {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.key, p.name, p.repo_url, p.issue_tracker_url, p.image, p.base_branch, \
             p.forge_credential_id, \
             EXISTS (SELECT 1 FROM tasks t WHERE t.project_id = p.id) AS has_tasks \
             FROM projects p ",
            $tail
        )
    };
}

/// `SELECT` of a persona with a trailing clause.
macro_rules! select_persona {
    ($tail:literal) => {
        concat!(
            "SELECT id, name, system_prompt, harness, adapter_command, model, \
             harness_credential_id FROM personas ",
            $tail
        )
    };
}

pub async fn list_credential_refs(pool: &PgPool) -> Result<Vec<CredentialRef>, DbError> {
    let rows: Vec<CredentialRow> = sqlx::query_as(
        "SELECT id, name, secret_name, secret_key FROM credential_refs ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_credential_ref(pool: &PgPool, id: Uuid) -> Result<Option<CredentialRef>, DbError> {
    let row: Option<CredentialRow> = sqlx::query_as(
        "SELECT id, name, secret_name, secret_key FROM credential_refs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectRecord>, DbError> {
    Ok(sqlx::query_as(select_project!("ORDER BY p.key"))
        .fetch_all(pool)
        .await?)
}

pub async fn get_project_by_key(
    pool: &PgPool,
    key: &str,
) -> Result<Option<ProjectRecord>, DbError> {
    Ok(sqlx::query_as(select_project!("WHERE p.key = $1"))
        .bind(key)
        .fetch_optional(pool)
        .await?)
}

pub async fn list_personas(pool: &PgPool) -> Result<Vec<PersonaRecord>, DbError> {
    Ok(sqlx::query_as(select_persona!("ORDER BY name"))
        .fetch_all(pool)
        .await?)
}

pub async fn get_persona(pool: &PgPool, id: Uuid) -> Result<Option<PersonaRecord>, DbError> {
    Ok(sqlx::query_as(select_persona!("WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await?)
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
            sqlx::query_as(
                "SELECT id, name, secret_name, secret_key FROM credential_refs WHERE id = $1 FOR UPDATE",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("credential reference `{id}`")))?,
        ),
        None => None,
    };
    let row: CredentialRow = match &before {
        Some(b) => sqlx::query_as(
            "UPDATE credential_refs SET name = $2, secret_name = $3, secret_key = $4, updated_at = now() \
             WHERE id = $1 RETURNING id, name, secret_name, secret_key",
        )
        .bind(b.id),
        None => sqlx::query_as(
            "INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, $3, $4) \
             RETURNING id, name, secret_name, secret_key",
        )
        .bind(Uuid::now_v7()),
    }
    .bind(&input.name)
    .bind(&input.secret_name)
    .bind(&input.secret_key)
    .fetch_one(&mut *tx)
    .await
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

/// Creates or updates a project and audits the change. Changing the key of a project that has
/// tasks is a [`DbError::Conflict`].
pub async fn save_project(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &ProjectInput,
) -> Result<ProjectRecord, DbError> {
    let mut tx = pool.begin().await?;
    let before: Option<ProjectRecord> = match id {
        Some(id) => {
            sqlx::query("SELECT 1 FROM projects WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("project `{id}`")))?;
            Some(
                sqlx::query_as(select_project!("WHERE p.id = $1"))
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await?,
            )
        }
        None => None,
    };
    if let Some(b) = &before
        && b.has_tasks
        && b.key != input.key
    {
        return Err(DbError::Conflict(
            "the key cannot change once the project has tasks".into(),
        ));
    }
    let row_id: Uuid = match &before {
        Some(b) => sqlx::query_scalar(
            "UPDATE projects SET key = $2, name = $3, repo_url = $4, issue_tracker_url = $5, \
             image = $6, base_branch = $7, forge_credential_id = $8, updated_at = now() \
             WHERE id = $1 RETURNING id",
        )
        .bind(b.id),
        None => sqlx::query_scalar(
            "INSERT INTO projects (id, key, name, repo_url, issue_tracker_url, image, base_branch, \
             forge_credential_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        )
        .bind(Uuid::now_v7()),
    }
    .bind(&input.key)
    .bind(&input.name)
    .bind(&input.repo_url)
    .bind(&input.issue_tracker_url)
    .bind(&input.image)
    .bind(&input.base_branch)
    .bind(input.forge_credential_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| conflict_or(e, "a project with this key already exists"))?;

    let record: ProjectRecord = sqlx::query_as(select_project!("WHERE p.id = $1"))
        .bind(row_id)
        .fetch_one(&mut *tx)
        .await?;
    let before_input = before.as_ref().map(project_input);
    audit(
        &mut tx,
        actor,
        "project",
        &record.key,
        record.id,
        before_input.as_ref(),
        input,
    )
    .await?;
    tx.commit().await?;
    Ok(record)
}

/// Creates or updates a persona and audits the change.
pub async fn save_persona(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &PersonaInput,
) -> Result<PersonaRecord, DbError> {
    let mut tx = pool.begin().await?;
    let before: Option<PersonaRecord> = match id {
        Some(id) => Some(
            sqlx::query_as(select_persona!("WHERE id = $1 FOR UPDATE"))
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("persona `{id}`")))?,
        ),
        None => None,
    };
    let record: PersonaRecord = match &before {
        Some(b) => sqlx::query_as(
            "UPDATE personas SET name = $2, system_prompt = $3, harness = $4, adapter_command = $5, \
             model = $6, harness_credential_id = $7, updated_at = now() WHERE id = $1 \
             RETURNING id, name, system_prompt, harness, adapter_command, model, harness_credential_id",
        )
        .bind(b.id),
        None => sqlx::query_as(
            "INSERT INTO personas (id, name, system_prompt, harness, adapter_command, model, \
             harness_credential_id) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING id, name, system_prompt, harness, adapter_command, model, harness_credential_id",
        )
        .bind(Uuid::now_v7()),
    }
    .bind(&input.name)
    .bind(&input.system_prompt)
    .bind(&input.harness)
    .bind(&input.adapter_command)
    .bind(&input.model)
    .bind(input.harness_credential_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| conflict_or(e, "a persona with this name already exists"))?;

    let before_input = before.map(|b| PersonaInput {
        name: b.name,
        system_prompt: b.system_prompt,
        harness: b.harness,
        adapter_command: b.adapter_command,
        model: b.model,
        harness_credential_id: b.harness_credential_id,
    });
    audit(
        &mut tx,
        actor,
        "persona",
        &record.name,
        record.id,
        before_input.as_ref(),
        input,
    )
    .await?;
    tx.commit().await?;
    Ok(record)
}

fn project_input(p: &ProjectRecord) -> ProjectInput {
    ProjectInput {
        key: p.key.clone(),
        name: p.name.clone(),
        repo_url: p.repo_url.clone(),
        issue_tracker_url: p.issue_tracker_url.clone(),
        image: p.image.clone(),
        base_branch: p.base_branch.clone(),
        forge_credential_id: p.forge_credential_id,
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
async fn audit<T: Serialize>(
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
mod tests {
    use super::*;
    use df_core::UserId;

    fn actor() -> (Principal, Vec<String>) {
        (
            Principal::human(UserId::new(), "Peter"),
            vec!["board-member".to_string()],
        )
    }

    fn project(key: &str, credential: Option<Uuid>) -> ProjectInput {
        ProjectInput {
            key: key.into(),
            name: "Dark Factory".into(),
            repo_url: "https://git.example.org/o/r.git".into(),
            issue_tracker_url: None,
            image: "img:latest".into(),
            base_branch: "main".into(),
            forge_credential_id: credential,
        }
    }

    fn persona(prompt: &str) -> PersonaInput {
        PersonaInput {
            name: "Linus".into(),
            system_prompt: prompt.into(),
            harness: "claude-code".into(),
            adapter_command: vec!["claude-code-acp".into()],
            model: None,
            harness_credential_id: None,
        }
    }

    async fn audit_details(pool: &PgPool, action: &str) -> Vec<serde_json::Value> {
        sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
            .bind(action)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn credential_refs_and_personas_are_audited_with_before_and_after() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (principal, roles) = actor();
        let actor = Actor {
            principal: &principal,
            roles: &roles,
        };

        let cred = save_credential_ref(
            &pool,
            actor,
            None,
            &CredentialRefInput {
                name: "claude-subscription".into(),
                secret_name: "claude".into(),
                secret_key: "token".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            list_credential_refs(&pool).await.unwrap(),
            vec![cred.clone()]
        );
        let created = audit_details(&pool, "credential_ref.create").await;
        assert_eq!(created[0]["after"]["secret_name"], "claude");
        assert!(created[0]["before"].is_null());

        let p = save_persona(&pool, actor, None, &persona("old"))
            .await
            .unwrap();
        save_persona(&pool, actor, Some(p.id), &persona("new"))
            .await
            .unwrap();
        let updated = audit_details(&pool, "persona.update").await;
        assert_eq!(updated[0]["before"]["system_prompt"], "old");
        assert_eq!(updated[0]["after"]["system_prompt"], "new");
        assert_eq!(
            get_persona(&pool, p.id)
                .await
                .unwrap()
                .unwrap()
                .system_prompt,
            "new"
        );

        let duplicate = save_persona(&pool, actor, None, &persona("x")).await;
        assert!(matches!(duplicate, Err(DbError::Conflict(_))));
    }

    #[tokio::test]
    async fn project_key_is_immutable_once_tasks_exist() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (principal, roles) = actor();
        let actor = Actor {
            principal: &principal,
            roles: &roles,
        };

        let p = save_project(&pool, actor, None, &project("DF", None))
            .await
            .unwrap();
        assert!(!p.has_tasks);
        // Without tasks the key may change.
        let renamed = save_project(&pool, actor, Some(p.id), &project("DFX", None))
            .await
            .unwrap();
        assert_eq!(renamed.key, "DFX");

        let persona = save_persona(&pool, actor, None, &persona("p"))
            .await
            .unwrap();
        crate::create_task(
            &pool,
            crate::NewTask {
                project_id: df_core::ProjectId::from_uuid(p.id),
                persona_id: df_core::PersonaId::from_uuid(persona.id),
                title: "t".into(),
                description: String::new(),
                issue_number: None,
                created_by: None,
            },
        )
        .await
        .unwrap();

        let rejected = save_project(&pool, actor, Some(p.id), &project("OTHER", None)).await;
        assert!(
            matches!(rejected, Err(DbError::Conflict(_))),
            "{rejected:?}"
        );
        let same_key = save_project(&pool, actor, Some(p.id), &project("DFX", None))
            .await
            .unwrap();
        assert!(same_key.has_tasks);
        assert_eq!(
            get_project_by_key(&pool, "DFX").await.unwrap().unwrap().id,
            p.id
        );
    }
}
