//! MCP servers (iter4-mcp-gateway design D2): an upstream HTTP MCP server with the credential
//! reference Dark Factory uses for it, the projects that enable it, and the calls of runs.
//!
//! Every change is audited in the same transaction and publishes `egress_policy`, so gateways
//! pick it up within about a second. A server never holds a secret value, only the id of a
//! credential reference. Its URL may carry a query, so no `Debug` output prints it.

use std::collections::HashMap;
use std::fmt;

use df_core::mcp::{McpCredentialScheme, url_without_query};
use serde::Serialize;
use sqlx::{FromRow, PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::{Actor, audit};
use crate::notify::{Notification, publish};
use crate::pool::DbError;

mod calls;

pub use calls::{McpCallRecord, insert_mcp_call, list_mcp_calls_of_runs};

/// Fields of an MCP server.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct McpServerInput {
    /// 1 to 20 characters, see `df_core::mcp::validate_name`; immutable.
    pub name: String,
    /// The upstream URL with path and query; not secret.
    pub url: String,
    pub credential_ref_id: Uuid,
    #[serde(flatten)]
    pub scheme: McpCredentialScheme,
    /// Skip the gateway's private-address check.
    pub private_network: bool,
}

impl fmt::Debug for McpServerInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpServerInput")
            .field("name", &self.name)
            .field("url", &url_without_query(&self.url))
            .field("credential_ref_id", &self.credential_ref_id)
            .field("scheme", &self.scheme)
            .field("private_network", &self.private_network)
            .finish()
    }
}

/// A stored MCP server.
#[derive(Clone, PartialEq, Eq)]
pub struct McpServerRecord {
    pub id: Uuid,
    pub input: McpServerInput,
    /// Increases on every edit.
    pub config_version: i64,
    /// Keys of the projects that enable the server, sorted; empty where the record is loaded
    /// for one project's runs.
    pub projects: Vec<String>,
}

impl fmt::Debug for McpServerRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpServerRecord")
            .field("id", &self.id)
            .field("input", &self.input)
            .field("config_version", &self.config_version)
            .field("projects", &self.projects)
            .finish()
    }
}

#[derive(FromRow)]
struct ServerRow {
    id: Uuid,
    name: String,
    url: String,
    credential_ref_id: Uuid,
    credential_scheme: String,
    credential_header: Option<String>,
    credential_prefix: Option<String>,
    credential_query_param: Option<String>,
    private_network: bool,
    config_version: i64,
    projects: Vec<String>,
}

impl TryFrom<ServerRow> for McpServerRecord {
    type Error = DbError;

    fn try_from(row: ServerRow) -> Result<Self, DbError> {
        let corrupt = |what: &str| DbError::InvalidData(format!("MCP server {}: {what}", row.id));
        let scheme = match row.credential_scheme.as_str() {
            "bearer" => McpCredentialScheme::Bearer,
            "header" => McpCredentialScheme::Header {
                name: row
                    .credential_header
                    .clone()
                    .ok_or_else(|| corrupt("no header name"))?,
                prefix: row.credential_prefix.clone().unwrap_or_default(),
            },
            "query" => McpCredentialScheme::Query {
                parameter: row
                    .credential_query_param
                    .clone()
                    .ok_or_else(|| corrupt("no query parameter"))?,
            },
            other => return Err(corrupt(&format!("unknown scheme `{other}`"))),
        };
        Ok(Self {
            id: row.id,
            input: McpServerInput {
                name: row.name,
                url: row.url,
                credential_ref_id: row.credential_ref_id,
                scheme,
                private_network: row.private_network,
            },
            config_version: row.config_version,
            projects: row.projects,
        })
    }
}

/// `SELECT` of servers with the keys of the projects that enable them, plus a trailing clause.
macro_rules! select_servers {
    ($tail:literal) => {
        concat!(
            "SELECT s.id, s.name, s.url, s.credential_ref_id, s.credential_scheme, \
             s.credential_header, s.credential_prefix, s.credential_query_param, \
             s.private_network, s.config_version, \
             COALESCE(array_agg(p.key ORDER BY p.key) FILTER (WHERE p.key IS NOT NULL), \
                      '{}') AS projects \
             FROM mcp_servers s \
             LEFT JOIN project_mcp_servers ps ON ps.server_id = s.id \
             LEFT JOIN projects p ON p.id = ps.project_id ",
            $tail
        )
    };
}

/// Every server, by name.
pub async fn list_mcp_servers(pool: &PgPool) -> Result<Vec<McpServerRecord>, DbError> {
    let rows: Vec<ServerRow> = sqlx::query_as(select_servers!("GROUP BY s.id ORDER BY s.name"))
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(McpServerRecord::try_from).collect()
}

pub async fn get_mcp_server(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<McpServerRecord>, DbError> {
    let row: Option<ServerRow> = sqlx::query_as(select_servers!("WHERE s.id = $1 GROUP BY s.id"))
        .bind(id)
        .fetch_optional(executor)
        .await?;
    row.map(McpServerRecord::try_from).transpose()
}

/// Creates (`id` is `None`) or updates a server and audits the change. An update increases
/// `config_version`. The name cannot change, the credential reference cannot be one a project
/// uses as its forge credential or a model provider uses (all [`DbError::Conflict`]).
pub async fn save_mcp_server(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &McpServerInput,
) -> Result<McpServerRecord, DbError> {
    let mut tx = pool.begin().await?;
    check_credential_free_of_factory_use(&mut *tx, input.credential_ref_id).await?;
    let before = match id {
        Some(id) => {
            sqlx::query_scalar::<_, i32>("SELECT 1 FROM mcp_servers WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("MCP server `{id}`")))?;
            get_mcp_server(&mut *tx, id).await?
        }
        None => None,
    };
    if let Some(before) = &before
        && before.input.name != input.name
    {
        return Err(DbError::Conflict(
            "the name cannot change after creation".into(),
        ));
    }
    let (header, prefix, query_param) = scheme_columns(&input.scheme);
    let saved_id = id.unwrap_or_else(Uuid::now_v7);
    let query = if before.is_some() {
        "UPDATE mcp_servers SET url = $3, credential_ref_id = $4, credential_scheme = $5, \
         credential_header = $6, credential_prefix = $7, credential_query_param = $8, \
         private_network = $9, config_version = config_version + 1, updated_at = now() \
         WHERE id = $1 AND name = $2"
    } else {
        "INSERT INTO mcp_servers (id, name, url, credential_ref_id, credential_scheme, \
         credential_header, credential_prefix, credential_query_param, private_network) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
    };
    sqlx::query(query)
        .bind(saved_id)
        .bind(&input.name)
        .bind(&input.url)
        .bind(input.credential_ref_id)
        .bind(input.scheme.as_str())
        .bind(header)
        .bind(prefix)
        .bind(query_param)
        .bind(input.private_network)
        .execute(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                DbError::Conflict("an MCP server with this name already exists".into())
            }
            _ => DbError::Sqlx(e),
        })?;
    audit(
        &mut tx,
        actor,
        "mcp_server",
        &input.name,
        saved_id,
        before.as_ref().map(|b| &b.input),
        input,
    )
    .await?;
    publish(&mut *tx, &Notification::EgressPolicy).await?;
    let record = get_mcp_server(&mut *tx, saved_id)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("MCP server `{saved_id}`")))?;
    tx.commit().await?;
    Ok(record)
}

/// Deletes a server no project enables and audits it. A server in use is a
/// [`DbError::Conflict`] naming the projects.
pub async fn delete_mcp_server(pool: &PgPool, actor: Actor<'_>, id: Uuid) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM mcp_servers WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("MCP server `{id}`")))?;
    let record = get_mcp_server(&mut *tx, id)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("MCP server `{id}`")))?;
    if !record.projects.is_empty() {
        return Err(DbError::Conflict(format!(
            "MCP server {} is enabled for {}; disable it there first.",
            record.input.name,
            record.projects.join(", ")
        )));
    }
    sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "mcp_server.delete",
        "mcp_server",
        &record.input.name,
    )
    .with_details(&serde_json::json!({ "id": id }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::EgressPolicy).await?;
    tx.commit().await?;
    Ok(())
}

/// The ids of the servers enabled for `project_id`.
pub async fn project_mcp_server_ids(
    executor: impl PgExecutor<'_>,
    project_id: Uuid,
) -> Result<Vec<Uuid>, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT ps.server_id FROM project_mcp_servers ps \
         JOIN mcp_servers s ON s.id = ps.server_id \
         WHERE ps.project_id = $1 ORDER BY s.name",
    )
    .bind(project_id)
    .fetch_all(executor)
    .await?)
}

/// Makes `server_ids` the servers enabled for the project and audits the change
/// (`project.mcp_servers_update`). A server id that does not exist is a [`DbError::NotFound`].
pub async fn set_project_mcp_servers(
    pool: &PgPool,
    actor: Actor<'_>,
    project_id: Uuid,
    server_ids: &[Uuid],
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    let key: String = sqlx::query_scalar("SELECT key FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("project `{project_id}`")))?;
    let before = enabled_names(&mut tx, project_id).await?;
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_servers WHERE id = ANY($1)")
        .bind(server_ids)
        .fetch_one(&mut *tx)
        .await?;
    let distinct: std::collections::HashSet<&Uuid> = server_ids.iter().collect();
    if usize::try_from(known).ok() != Some(distinct.len()) {
        return Err(DbError::NotFound("MCP server".into()));
    }
    sqlx::query("DELETE FROM project_mcp_servers WHERE project_id = $1 AND server_id <> ALL($2)")
        .bind(project_id)
        .bind(server_ids)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO project_mcp_servers (project_id, server_id) \
         SELECT $1, id FROM mcp_servers WHERE id = ANY($2) ON CONFLICT DO NOTHING",
    )
    .bind(project_id)
    .bind(server_ids)
    .execute(&mut *tx)
    .await?;
    let after = enabled_names(&mut tx, project_id).await?;
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "project.mcp_servers_update",
        "project",
        &key,
    )
    .with_details(&serde_json::json!({ "id": project_id, "before": before, "after": after }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(&mut *tx, &Notification::EgressPolicy).await?;
    tx.commit().await?;
    Ok(())
}

async fn enabled_names(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<Vec<String>, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT s.name FROM project_mcp_servers ps JOIN mcp_servers s ON s.id = ps.server_id \
         WHERE ps.project_id = $1 ORDER BY s.name",
    )
    .bind(project_id)
    .fetch_all(&mut **tx)
    .await?)
}

/// The servers enabled for each of `project_ids` (`projects` of the records is empty). Projects
/// without an enabled server are left out.
pub async fn enabled_servers_of_projects(
    pool: &PgPool,
    project_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<McpServerRecord>>, DbError> {
    let pairs: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT ps.project_id, ps.server_id FROM project_mcp_servers ps \
         JOIN mcp_servers s ON s.id = ps.server_id WHERE ps.project_id = ANY($1) \
         ORDER BY s.name",
    )
    .bind(project_ids)
    .fetch_all(pool)
    .await?;
    let mut servers: HashMap<Uuid, McpServerRecord> = HashMap::new();
    let wanted: Vec<Uuid> = pairs.iter().map(|(_, server)| *server).collect();
    let rows: Vec<ServerRow> =
        sqlx::query_as(select_servers!("WHERE s.id = ANY($1) GROUP BY s.id"))
            .bind(&wanted)
            .fetch_all(pool)
            .await?;
    for row in rows {
        let mut record = McpServerRecord::try_from(row)?;
        record.projects.clear();
        servers.insert(record.id, record);
    }
    let mut by_project: HashMap<Uuid, Vec<McpServerRecord>> = HashMap::new();
    for (project, server) in pairs {
        if let Some(record) = servers.get(&server) {
            by_project.entry(project).or_default().push(record.clone());
        }
    }
    Ok(by_project)
}

/// Names of the servers that use credential reference `credential_ref_id`, sorted.
pub async fn servers_using_credential(
    executor: impl PgExecutor<'_>,
    credential_ref_id: Uuid,
) -> Result<Vec<String>, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT name FROM mcp_servers WHERE credential_ref_id = $1 ORDER BY name",
    )
    .bind(credential_ref_id)
    .fetch_all(executor)
    .await?)
}

/// Refuses a credential reference that an MCP server uses, naming the servers: projects and
/// model providers must not share a credential with one (the other direction of
/// [`save_mcp_server`]'s check).
pub(crate) async fn check_credential_not_used_by_a_server(
    conn: &mut sqlx::PgConnection,
    credential_ref_id: Uuid,
) -> Result<(), DbError> {
    let users = servers_using_credential(&mut *conn, credential_ref_id).await?;
    if users.is_empty() {
        return Ok(());
    }
    let credential: String = sqlx::query_scalar("SELECT name FROM credential_refs WHERE id = $1")
        .bind(credential_ref_id)
        .fetch_one(&mut *conn)
        .await?;
    Err(DbError::Conflict(format!(
        "credential `{credential}` is used by MCP server {}",
        users.join(", ")
    )))
}

/// Refuses a credential reference that a project uses as its forge credential or that a model
/// provider uses, naming the first user found.
async fn check_credential_free_of_factory_use(
    executor: &mut sqlx::PgConnection,
    credential_ref_id: Uuid,
) -> Result<(), DbError> {
    let project: Option<(String, String)> = sqlx::query_as(
        "SELECT c.name, p.key FROM projects p JOIN credential_refs c ON c.id = p.forge_credential_id \
         WHERE c.id = $1 ORDER BY p.key LIMIT 1",
    )
    .bind(credential_ref_id)
    .fetch_optional(&mut *executor)
    .await?;
    if let Some((credential, key)) = project {
        return Err(DbError::Conflict(format!(
            "credential `{credential}` is the forge credential of project {key}"
        )));
    }
    let provider: Option<(String, String)> = sqlx::query_as(
        "SELECT c.name, m.name FROM model_providers m JOIN credential_refs c ON c.id = m.credential_ref_id \
         WHERE c.id = $1 ORDER BY m.name LIMIT 1",
    )
    .bind(credential_ref_id)
    .fetch_optional(&mut *executor)
    .await?;
    if let Some((credential, name)) = provider {
        return Err(DbError::Conflict(format!(
            "credential `{credential}` is the credential of model provider {name}"
        )));
    }
    Ok(())
}

fn scheme_columns(scheme: &McpCredentialScheme) -> (Option<&str>, Option<&str>, Option<&str>) {
    match scheme {
        McpCredentialScheme::Bearer => (None, None, None),
        McpCredentialScheme::Header { name, prefix } => (
            Some(name.as_str()),
            Some(prefix.as_str()).filter(|prefix| !prefix.is_empty()),
            None,
        ),
        McpCredentialScheme::Query { parameter } => (None, None, Some(parameter.as_str())),
    }
}

#[cfg(test)]
mod tests;
