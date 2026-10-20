//! Credentials a run receives: the harness token and a Git credential for the project's
//! repository only (`specs/run-credentials/spec.md`). Secret values are never logged or stored.

use std::collections::HashMap;

use df_core::CredentialRef;
use df_proto::agent::v1::{
    CredentialGrant, CredentialRequest, GitCredential, GitCredentialRequest, HarnessCredentials,
    credential_grant, credential_request,
};
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

/// Answers a `CredentialRequest`: Git credentials only, only for the project's repository.
pub async fn grant(
    ctx: &ChannelContext,
    rc: &RunContext,
    request: CredentialRequest,
) -> CredentialGrant {
    let result = match request.kind {
        Some(credential_request::Kind::Git(git)) => git_credential(ctx, rc, &git).await,
        None => Err("unsupported credential request".to_string()),
    };
    CredentialGrant {
        request_id: request.request_id,
        result: Some(match result {
            Ok(credential) => credential_grant::Result::Git(credential),
            Err(reason) => credential_grant::Result::DeniedReason(reason),
        }),
    }
}

/// The answer to a request that arrives after the run ended; no credential is handed out.
pub fn run_ended(request_id: String) -> CredentialGrant {
    CredentialGrant {
        request_id,
        result: Some(credential_grant::Result::DeniedReason(
            "the run has ended".to_string(),
        )),
    }
}

async fn git_credential(
    ctx: &ChannelContext,
    rc: &RunContext,
    request: &GitCredentialRequest,
) -> Result<GitCredential, String> {
    if !matches_repository(&rc.project.repo_url, request) {
        return Err("credentials are only available for the project's repository".to_string());
    }
    let reference: &CredentialRef = rc
        .project
        .forge_credential
        .as_ref()
        .ok_or_else(|| "the project has no forge credential".to_string())?;
    let token = ctx.secrets.get(reference).await.map_err(|e| {
        tracing::warn!(error = %e, credential = %reference.name, "forge credential unavailable");
        "the forge credential is unavailable".to_string()
    })?;
    Ok(GitCredential {
        username: ctx.config.agent.git_username.clone(),
        password: token.expose_secret().to_string(),
        expires_at: None,
    })
}

/// Whether protocol, host and path of `request` name `repo_url` (a trailing `.git` is optional).
fn matches_repository(repo_url: &str, request: &GitCredentialRequest) -> bool {
    let Some((scheme, rest)) = repo_url.split_once("://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let normalize = |p: &str| {
        let p = p.trim_matches('/');
        p.strip_suffix(".git").unwrap_or(p).to_string()
    };
    request.protocol.eq_ignore_ascii_case(scheme)
        && request.host.eq_ignore_ascii_case(host)
        && !request.path.is_empty()
        && normalize(&request.path) == normalize(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::load_run_context;
    use crate::runs::get_run;
    use crate::testing::Env;
    use df_core::SecretError;
    use sqlx::PgPool;

    fn git(protocol: &str, host: &str, path: &str) -> CredentialRequest {
        CredentialRequest {
            request_id: "r1".into(),
            kind: Some(credential_request::Kind::Git(GitCredentialRequest {
                protocol: protocol.into(),
                host: host.into(),
                path: path.into(),
            })),
        }
    }

    async fn context(pool: PgPool) -> (Env, RunContext) {
        let env = Env::new(pool.clone()).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
            .await
            .unwrap();
        (env, rc)
    }

    #[test]
    fn repository_matching_is_strict_about_host_and_path() {
        let url = "https://git.example.org/peter/dark-factory.git";
        let req = |p: &str, h: &str, path: &str| GitCredentialRequest {
            protocol: p.into(),
            host: h.into(),
            path: path.into(),
        };
        assert!(matches_repository(
            url,
            &req("https", "git.example.org", "peter/dark-factory.git")
        ));
        assert!(matches_repository(
            url,
            &req("https", "git.example.org", "peter/dark-factory")
        ));
        assert!(!matches_repository(
            url,
            &req("https", "git.example.org", "peter/other.git")
        ));
        assert!(!matches_repository(
            url,
            &req("http", "git.example.org", "peter/dark-factory.git")
        ));
        assert!(!matches_repository(
            url,
            &req("https", "evil.example.org", "peter/dark-factory.git")
        ));
        assert!(!matches_repository(
            url,
            &req("https", "git.example.org", "")
        ));
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

    #[tokio::test]
    async fn git_credentials_only_for_the_project_repository() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = context(pool).await;

        let granted = grant(
            &env.ctx,
            &rc,
            git("https", "git.example.org", "peter/dark-factory.git"),
        )
        .await;
        assert!(
            matches!(&granted.result, Some(credential_grant::Result::Git(c))
                if c.username == "dark-factory" && c.password == "forge-secret-value"),
            "expected the forge credential, got {granted:?}"
        );

        let denied = grant(
            &env.ctx,
            &rc,
            git("https", "git.example.org", "peter/other.git"),
        )
        .await;
        assert!(
            matches!(denied.result, Some(credential_grant::Result::DeniedReason(r)) if !r.is_empty())
        );
        assert_eq!(denied.request_id, "r1");
    }
}
