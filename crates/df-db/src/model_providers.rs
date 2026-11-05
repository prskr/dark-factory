//! Model providers (iter3-inference-gateway design D5): an upstream model service with its wire
//! APIs and the credential reference Dark Factory uses for it. Personas reference one.
//!
//! Every change is audited in the same transaction. A provider never holds a secret value, only
//! the id of a credential reference.

use std::collections::HashMap;

use df_core::model_provider::{
    CredentialScheme, HeaderTemplate, ModelLimits, ProviderApi, ProviderModel, WireApi,
};
use serde::Serialize;
use sqlx::types::Json;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::{Actor, audit};
use crate::pool::DbError;

/// Fields of a model provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelProviderInput {
    pub name: String,
    pub credential_ref_id: Uuid,
    pub scheme: CredentialScheme,
    /// Header name of [`CredentialScheme::Header`].
    pub credential_header: Option<String>,
    /// Text put before the credential value.
    pub credential_prefix: Option<String>,
    pub headers: Vec<HeaderTemplate>,
    pub apis: Vec<ProviderApi>,
    /// Sorted by model ID.
    pub models: Vec<ProviderModel>,
}

/// A stored model provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProviderRecord {
    pub id: Uuid,
    pub input: ModelProviderInput,
}

impl ModelProviderRecord {
    /// The wire APIs the provider offers.
    pub fn wire_apis(&self) -> Vec<WireApi> {
        self.input.apis.iter().map(|api| api.wire_api).collect()
    }

    /// The model with `model_id`.
    pub fn model(&self, model_id: &str) -> Option<&ProviderModel> {
        self.input
            .models
            .iter()
            .find(|model| model.model_id == model_id)
    }
}

/// The provider and model of an active run and the harness that uses them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunModelProvider {
    pub harness: String,
    pub provider: ModelProviderRecord,
    pub model: ProviderModel,
}

#[derive(FromRow)]
struct ProviderRow {
    id: Uuid,
    name: String,
    credential_ref_id: Uuid,
    credential_scheme: String,
    credential_header: Option<String>,
    credential_prefix: Option<String>,
    headers: Json<Vec<HeaderTemplate>>,
}

#[derive(FromRow)]
struct ModelRow {
    provider_id: Uuid,
    model_id: String,
    wire_api: String,
    context_window_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
}

#[derive(FromRow)]
struct ApiRow {
    provider_id: Uuid,
    wire_api: String,
    upstream_url: String,
}

/// `SELECT` of a provider with a trailing clause.
macro_rules! select_provider {
    ($tail:literal) => {
        concat!(
            "SELECT id, name, credential_ref_id, credential_scheme, credential_header, \
             credential_prefix, headers FROM model_providers ",
            $tail
        )
    };
}

pub async fn list_model_providers(pool: &PgPool) -> Result<Vec<ModelProviderRecord>, DbError> {
    let rows: Vec<ProviderRow> = sqlx::query_as(select_provider!("ORDER BY name"))
        .fetch_all(pool)
        .await?;
    assemble(pool, rows).await
}

pub async fn get_model_provider(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<ModelProviderRecord>, DbError> {
    let rows: Vec<ProviderRow> = sqlx::query_as(select_provider!("WHERE id = $1"))
        .bind(id)
        .fetch_all(pool)
        .await?;
    Ok(assemble(pool, rows).await?.pop())
}

/// Names of the personas that use the provider, sorted.
pub async fn personas_using(pool: &PgPool, id: Uuid) -> Result<Vec<String>, DbError> {
    Ok(
        sqlx::query_scalar("SELECT name FROM personas WHERE model_provider_id = $1 ORDER BY name")
            .bind(id)
            .fetch_all(pool)
            .await?,
    )
}

/// Creates (`id` is `None`) or updates a provider with its APIs and audits the change.
pub async fn save_model_provider(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Option<Uuid>,
    input: &ModelProviderInput,
) -> Result<ModelProviderRecord, DbError> {
    let mut tx = pool.begin().await?;
    let before = match id {
        Some(id) => {
            let row: ProviderRow = sqlx::query_as(select_provider!("WHERE id = $1 FOR UPDATE"))
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("model provider `{id}`")))?;
            let apis = load_apis(&mut *tx, &[id]).await?;
            let models = load_models(&mut *tx, &[id]).await?;
            Some(record(row, &apis, &models)?.input)
        }
        None => None,
    };
    let id = id.unwrap_or_else(Uuid::now_v7);
    let headers = Json(&input.headers);
    let query = if before.is_some() {
        "UPDATE model_providers SET name = $2, credential_ref_id = $3, credential_scheme = $4, \
         credential_header = $5, credential_prefix = $6, headers = $7, updated_at = now() \
         WHERE id = $1"
    } else {
        "INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme, \
         credential_header, credential_prefix, headers) VALUES ($1, $2, $3, $4, $5, $6, $7)"
    };
    sqlx::query(query)
        .bind(id)
        .bind(&input.name)
        .bind(input.credential_ref_id)
        .bind(input.scheme.as_str())
        .bind(&input.credential_header)
        .bind(&input.credential_prefix)
        .bind(headers)
        .execute(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                DbError::Conflict("a model provider with this name already exists".into())
            }
            _ => DbError::Sqlx(e),
        })?;
    replace_apis_and_models(&mut tx, id, input).await?;
    audit(
        &mut tx,
        actor,
        "model_provider",
        &input.name,
        id,
        before.as_ref(),
        input,
    )
    .await?;
    tx.commit().await?;
    Ok(ModelProviderRecord {
        id,
        input: input.clone(),
    })
}

/// Writes the APIs and models of the input and removes the rest. A model that a persona uses
/// cannot be removed, and neither can an API that a model uses ([`DbError::ModelConflict`] naming the
/// personas or models).
async fn replace_apis_and_models(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    input: &ModelProviderInput,
) -> Result<(), DbError> {
    let kept_models: Vec<&str> = input.models.iter().map(|m| m.model_id.as_str()).collect();
    let removed: Vec<String> = sqlx::query_scalar(
        "SELECT model_id FROM model_provider_models \
         WHERE provider_id = $1 AND model_id <> ALL($2) ORDER BY model_id",
    )
    .bind(id)
    .bind(&kept_models)
    .fetch_all(&mut **tx)
    .await?;
    for model_id in &removed {
        let users = personas_of_model(&mut **tx, id, model_id).await?;
        if !users.is_empty() {
            return Err(DbError::ModelConflict(format!(
                "{model_id} cannot be removed: persona {} uses it.",
                users.join(", ")
            )));
        }
    }
    sqlx::query("DELETE FROM model_provider_models WHERE provider_id = $1 AND model_id <> ALL($2)")
        .bind(id)
        .bind(&kept_models)
        .execute(&mut **tx)
        .await?;

    for api in &input.apis {
        sqlx::query(
            "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (provider_id, wire_api) DO UPDATE SET upstream_url = EXCLUDED.upstream_url",
        )
        .bind(id)
        .bind(api.wire_api.as_str())
        .bind(&api.upstream_url)
        .execute(&mut **tx)
        .await?;
    }
    let kept_apis: Vec<&str> = input.apis.iter().map(|api| api.wire_api.as_str()).collect();
    let blocking: Vec<&str> = input
        .models
        .iter()
        .filter(|m| !kept_apis.contains(&m.wire_api.as_str()))
        .map(|m| m.model_id.as_str())
        .collect();
    if !blocking.is_empty() {
        return Err(DbError::ModelConflict(format!(
            "an API cannot be removed: model {} uses it.",
            blocking.join(", ")
        )));
    }
    for model in &input.models {
        sqlx::query(
            "INSERT INTO model_provider_models \
             (provider_id, model_id, wire_api, context_window_tokens, max_output_tokens) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (provider_id, model_id) DO UPDATE SET wire_api = EXCLUDED.wire_api, \
             context_window_tokens = EXCLUDED.context_window_tokens, \
             max_output_tokens = EXCLUDED.max_output_tokens",
        )
        .bind(id)
        .bind(&model.model_id)
        .bind(model.wire_api.as_str())
        .bind(model.limits.map(|l| l.context_window_tokens))
        .bind(model.limits.map(|l| l.max_output_tokens))
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query("DELETE FROM model_provider_apis WHERE provider_id = $1 AND wire_api <> ALL($2)")
        .bind(id)
        .bind(&kept_apis)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn personas_of_model<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    id: Uuid,
    model_id: &str,
) -> Result<Vec<String>, DbError> {
    Ok(sqlx::query_scalar(
        "SELECT name FROM personas WHERE model_provider_id = $1 AND model = $2 ORDER BY name",
    )
    .bind(id)
    .bind(model_id)
    .fetch_all(executor)
    .await?)
}

/// Deletes a provider no persona uses and audits it. A provider in use is a
/// [`DbError::Conflict`] naming the personas.
pub async fn delete_model_provider(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Uuid,
) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    let name: String =
        sqlx::query_scalar("SELECT name FROM model_providers WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("model provider `{id}`")))?;
    let users: Vec<String> =
        sqlx::query_scalar("SELECT name FROM personas WHERE model_provider_id = $1 ORDER BY name")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if !users.is_empty() {
        return Err(DbError::Conflict(format!(
            "model provider `{name}` is used by persona {}",
            users.join(", ")
        )));
    }
    sqlx::query("DELETE FROM model_providers WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "model_provider.delete",
        "model_provider",
        &name,
    )
    .with_details(&serde_json::json!({ "id": id }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(())
}

/// The harness, model provider and model of each of the runs `run_ids`; runs that don't exist
/// are left out.
pub async fn run_model_providers(
    pool: &PgPool,
    run_ids: &[Uuid],
) -> Result<HashMap<Uuid, RunModelProvider>, DbError> {
    let rows: Vec<(Uuid, String, Uuid, String)> = sqlx::query_as(
        "SELECT r.id, pe.harness, pe.model_provider_id, pe.model FROM runs r \
         JOIN tasks t ON t.id = r.task_id JOIN personas pe ON pe.id = t.persona_id \
         WHERE r.id = ANY($1)",
    )
    .bind(run_ids)
    .fetch_all(pool)
    .await?;
    let providers: HashMap<Uuid, ModelProviderRecord> = list_model_providers(pool)
        .await?
        .into_iter()
        .map(|provider| (provider.id, provider))
        .collect();
    Ok(rows
        .into_iter()
        .filter_map(|(run_id, harness, provider_id, model_id)| {
            let provider = providers.get(&provider_id)?.clone();
            let model = provider.model(&model_id)?.clone();
            Some((
                run_id,
                RunModelProvider {
                    harness,
                    provider,
                    model,
                },
            ))
        })
        .collect())
}

async fn assemble(
    pool: &PgPool,
    rows: Vec<ProviderRow>,
) -> Result<Vec<ModelProviderRecord>, DbError> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let apis = load_apis(pool, &ids).await?;
    let models = load_models(pool, &ids).await?;
    rows.into_iter()
        .map(|row| record(row, &apis, &models))
        .collect()
}

async fn load_apis<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    ids: &[Uuid],
) -> Result<Vec<ApiRow>, DbError> {
    Ok(sqlx::query_as(
        "SELECT provider_id, wire_api, upstream_url FROM model_provider_apis \
         WHERE provider_id = ANY($1) ORDER BY wire_api",
    )
    .bind(ids)
    .fetch_all(executor)
    .await?)
}

async fn load_models<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    ids: &[Uuid],
) -> Result<Vec<ModelRow>, DbError> {
    Ok(sqlx::query_as(
        "SELECT provider_id, model_id, wire_api, context_window_tokens, max_output_tokens \
         FROM model_provider_models WHERE provider_id = ANY($1) ORDER BY model_id",
    )
    .bind(ids)
    .fetch_all(executor)
    .await?)
}

fn record(
    row: ProviderRow,
    apis: &[ApiRow],
    models: &[ModelRow],
) -> Result<ModelProviderRecord, DbError> {
    let apis = apis
        .iter()
        .filter(|api| api.provider_id == row.id)
        .map(|api| {
            Ok(ProviderApi {
                wire_api: api.wire_api.parse()?,
                upstream_url: api.upstream_url.clone(),
            })
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    let models = models
        .iter()
        .filter(|model| model.provider_id == row.id)
        .map(|model| {
            Ok(ProviderModel {
                model_id: model.model_id.clone(),
                wire_api: model.wire_api.parse()?,
                limits: model
                    .context_window_tokens
                    .zip(model.max_output_tokens)
                    .map(|(context_window_tokens, max_output_tokens)| ModelLimits {
                        context_window_tokens,
                        max_output_tokens,
                    }),
            })
        })
        .collect::<Result<Vec<_>, DbError>>()?;
    Ok(ModelProviderRecord {
        id: row.id,
        input: ModelProviderInput {
            name: row.name,
            credential_ref_id: row.credential_ref_id,
            scheme: row.credential_scheme.parse()?,
            credential_header: row.credential_header,
            credential_prefix: row.credential_prefix,
            headers: row.headers.0,
            apis,
            models,
        },
    })
}

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
pub(crate) mod tests;
