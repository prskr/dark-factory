//! Content-addressed PostgreSQL blob store (`blobs` table, keyed by SHA-256).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::pool::DbError;

/// Stored binary blob row from `blobs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobRow {
    pub sha256: String,
    pub mime_type: String,
    pub size: i64,
    pub data: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// Computes the lowercase hex-encoded SHA-256 digest of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex::encode(digest)
}

/// Content-addressed blob store backed by PostgreSQL (`blobs` table).
#[derive(Clone)]
pub struct PgBlobStore {
    pool: PgPool,
}

impl PgBlobStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Stores `data` with `mime_type` keyed by its SHA-256 digest.
    /// Idempotent: storing identical bytes a second time is a no-op and returns the same digest.
    pub async fn put(&self, mime_type: &str, data: &[u8]) -> Result<String, DbError> {
        let sha256 = sha256_hex(data);
        let size = i64::try_from(data.len()).unwrap_or(i64::MAX);

        sqlx::query!(
            r#"
            INSERT INTO blobs (sha256, mime_type, size, data)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (sha256) DO NOTHING
            "#,
            sha256,
            mime_type,
            size,
            data
        )
        .execute(&self.pool)
        .await?;

        Ok(sha256)
    }

    /// Retrieves a blob by its lowercase hex-encoded SHA-256 digest.
    pub async fn get(&self, sha256: &str) -> Result<Option<BlobRow>, DbError> {
        let row = sqlx::query!(
            r#"
            SELECT sha256, mime_type, size, data, created_at
            FROM blobs
            WHERE sha256 = $1
            "#,
            sha256
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| BlobRow {
            sha256: r.sha256,
            mime_type: r.mime_type,
            size: r.size,
            data: r.data,
            created_at: r.created_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blob_store_put_and_get_are_idempotent_and_content_addressed() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let store = PgBlobStore::new(pool);
        let payload = b"\x89PNG\r\n\x1a\nfake-image-bytes";

        let hash1 = store.put("image/png", payload).await.unwrap();
        let hash2 = store.put("image/png", payload).await.unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);

        let fetched = store.get(&hash1).await.unwrap().expect("blob should exist");
        assert_eq!(fetched.sha256, hash1);
        assert_eq!(fetched.mime_type, "image/png");
        assert_eq!(fetched.size, payload.len() as i64);
        assert_eq!(fetched.data, payload);

        let missing = store
            .get("0000000000000000000000000000000000000000000000000000000000000000")
            .await
            .unwrap();
        assert!(missing.is_none());
    }
}
