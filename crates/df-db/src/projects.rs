//! Project lookups for the forge integration.

use df_core::{CredentialRef, CredentialRefId, ProjectId};
use sqlx::PgPool;

use crate::pool::DbError;

/// A project's repository and the credential its forge token is read with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeTarget {
    pub repo_url: String,
    /// `None` if the project has no (complete) forge credential.
    pub credential: Option<CredentialRef>,
}

/// Reads the forge details of `project`; `None` if the project does not exist.
pub async fn forge_target(
    pool: &PgPool,
    project: &ProjectId,
) -> Result<Option<ForgeTarget>, DbError> {
    let row = sqlx::query!(
        "SELECT p.repo_url, c.id AS credential_id, c.name AS credential_name,
                c.secret_name, c.secret_key
         FROM projects p LEFT JOIN credential_refs c ON c.id = p.forge_credential_id
         WHERE p.id = $1",
        project.as_uuid()
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ForgeTarget {
        repo_url: row.repo_url,
        credential: match (
            row.credential_id,
            row.credential_name,
            row.secret_name,
            row.secret_key,
        ) {
            (Some(id), Some(name), Some(secret_name), Some(secret_key)) => Some(CredentialRef {
                id: CredentialRefId::from_uuid(id),
                name,
                secret_name,
                secret_key,
            }),
            _ => None,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    async fn insert_project(pool: &PgPool, key: &str, credential: Option<Uuid>) -> ProjectId {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image, forge_credential_id) \
             VALUES ($1, $2, $2, 'https://git.example.org/o/r.git', 'img:latest', $3)",
        )
        .bind(id)
        .bind(key)
        .bind(credential)
        .execute(pool)
        .await
        .unwrap();
        ProjectId::from_uuid(id)
    }

    // Needs a PostgreSQL container (testcontainers).
    #[tokio::test]
    async fn forge_target_is_found_with_and_without_credential() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let credential = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO credential_refs (id, name, secret_name, secret_key) \
             VALUES ($1, 'forge', 'forge-token', 'token')",
        )
        .bind(credential)
        .execute(&pool)
        .await
        .unwrap();

        let with = insert_project(&pool, "WITH", Some(credential)).await;
        let target = forge_target(&pool, &with).await.unwrap().unwrap();
        assert_eq!(target.repo_url, "https://git.example.org/o/r.git");
        let c = target.credential.unwrap();
        assert_eq!(
            (c.secret_name.as_str(), c.secret_key.as_str()),
            ("forge-token", "token")
        );

        let without = insert_project(&pool, "WITHOUT", None).await;
        let target = forge_target(&pool, &without).await.unwrap().unwrap();
        assert!(target.credential.is_none());

        assert!(
            forge_target(&pool, &ProjectId::new())
                .await
                .unwrap()
                .is_none()
        );
    }
}
