//! Human users, created on first login (`specs/human-auth/spec.md`).

use df_core::UserId;
use sqlx::PgPool;
use uuid::Uuid;

use crate::pool::DbError;

/// Inserts the user identified by `issuer` and `subject`, or refreshes display name and
/// last-seen time of the existing one.
pub async fn upsert_user(
    pool: &PgPool,
    issuer: &str,
    subject: &str,
    display_name: &str,
) -> Result<UserId, DbError> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, display_name) VALUES ($1, $2, $3, $4)
         ON CONFLICT (issuer, subject)
         DO UPDATE SET display_name = EXCLUDED.display_name, last_seen = now()
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(issuer)
    .bind(subject)
    .bind(display_name)
    .fetch_one(pool)
    .await?;
    Ok(UserId::from_uuid(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_creates_once_and_refreshes_name_and_last_seen() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let first = upsert_user(&pool, "https://kc/realms/df", "sub-1", "Peter")
            .await
            .unwrap();
        let (seen,): (chrono::DateTime<chrono::Utc>,) =
            sqlx::query_as("SELECT last_seen FROM users WHERE id = $1")
                .bind(first.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        let again = upsert_user(&pool, "https://kc/realms/df", "sub-1", "Peter K.")
            .await
            .unwrap();
        assert_eq!(first, again);
        let (name, seen_again): (String, chrono::DateTime<chrono::Utc>) =
            sqlx::query_as("SELECT display_name, last_seen FROM users WHERE id = $1")
                .bind(first.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "Peter K.");
        assert!(seen_again >= seen);
        let other = upsert_user(&pool, "https://kc/realms/df", "sub-2", "Anna")
            .await
            .unwrap();
        assert_ne!(other, first);
    }
}
