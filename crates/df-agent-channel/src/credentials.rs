//! Credentials a run receives: the harness token only. The gateway's Git proxy injects the forge
//! credential (`specs/run-credentials/spec.md`). Secret values are never logged or stored.

use std::collections::HashMap;

use df_proto::agent::v1::HarnessCredentials;
use secrecy::ExposeSecret;
use thiserror::Error;

use crate::ChannelContext;
use crate::assign::RunContext;

/// Environment variable the Claude Code harness reads its subscription token from.
const CLAUDE_TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// The persona's harness credential could not be resolved.
#[derive(Debug, Error)]
#[error(
    "harness credential `{credential}` is unavailable: secret `{secret_name}` key `{secret_key}` could not be read"
)]
pub struct MissingCredential {
    pub credential: String,
    pub secret_name: String,
    pub secret_key: String,
}

/// Resolves the persona's harness credential (`claude-code`: `CLAUDE_CODE_OAUTH_TOKEN`).
pub async fn harness_credentials(
    ctx: &ChannelContext,
    rc: &RunContext,
) -> Result<HarnessCredentials, MissingCredential> {
    let mut env = HashMap::new();
    if let Some(reference) = &rc.persona.harness_credential {
        let value = ctx.secrets.get(reference).await.map_err(|e| {
            tracing::warn!(error = %e, credential = %reference.name, "harness credential unavailable");
            MissingCredential {
                credential: reference.name.clone(),
                secret_name: reference.secret_name.clone(),
                secret_key: reference.secret_key.clone(),
            }
        })?;
        env.insert(
            CLAUDE_TOKEN_ENV.to_string(),
            value.expose_secret().to_string(),
        );
    }
    Ok(HarnessCredentials { env })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::load_run_context;
    use crate::runs::get_run;
    use crate::testing::Env;
    use df_core::SecretError;
    use sqlx::PgPool;

    async fn context(pool: PgPool) -> (Env, RunContext) {
        let env = Env::new(pool.clone()).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
            .await
            .unwrap();
        (env, rc)
    }

    #[tokio::test]
    async fn claude_token_is_delivered_as_env() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = context(pool).await;
        let creds = harness_credentials(&env.ctx, &rc).await.unwrap();
        assert_eq!(creds.env["CLAUDE_CODE_OAUTH_TOKEN"], "claude-secret-value");
    }

    #[tokio::test]
    async fn missing_secret_names_the_reference_not_the_value() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = context(pool).await;
        env.secrets.insert_error(
            "claude",
            "token",
            SecretError::NotFound {
                secret_name: "claude".into(),
                secret_key: "token".into(),
            },
        );
        let err = harness_credentials(&env.ctx, &rc).await.unwrap_err();
        assert_eq!(err.credential, "claude");
        assert!(!err.to_string().contains("claude-secret-value"));
    }
}
