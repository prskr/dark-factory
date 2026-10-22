//! PostgreSQL-backed `tower-sessions` store (table `web_sessions`, migration in `df-db`).
//!
//! Sessions survive a restart of the control plane. Expired rows are removed whenever a new
//! session is created.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tower_sessions::cookie::time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{Error, Result, SessionStore};

/// Session store on the `web_sessions` table.
#[derive(Debug, Clone)]
pub struct PgSessionStore {
    pool: PgPool,
}

impl PgSessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn insert(&self, record: &Record, overwrite: bool) -> Result<u64> {
        let data = serde_json::to_value(&record.data).map_err(|e| Error::Encode(e.to_string()))?;
        let expires_at = DateTime::<Utc>::from_timestamp(record.expiry_date.unix_timestamp(), 0)
            .ok_or_else(|| Error::Encode("expiry out of range".into()))?;
        let sql = if overwrite {
            "INSERT INTO web_sessions (id, data, expires_at) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET data = EXCLUDED.data, expires_at = EXCLUDED.expires_at"
        } else {
            "INSERT INTO web_sessions (id, data, expires_at) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO NOTHING"
        };
        let result = sqlx::query(sql)
            .bind(record.id.to_string())
            .bind(data)
            .bind(expires_at)
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(result.rows_affected())
    }
}

fn backend(err: sqlx::Error) -> Error {
    Error::Backend(err.to_string())
}

#[async_trait]
impl SessionStore for PgSessionStore {
    async fn create(&self, record: &mut Record) -> Result<()> {
        sqlx::query("DELETE FROM web_sessions WHERE expires_at < now()")
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        // A colliding id (practically impossible) is replaced by a fresh one.
        while self.insert(record, false).await? == 0 {
            record.id = Id::default();
        }
        Ok(())
    }

    async fn save(&self, record: &Record) -> Result<()> {
        self.insert(record, true).await.map(drop)
    }

    async fn load(&self, session_id: &Id) -> Result<Option<Record>> {
        let row: Option<(serde_json::Value, DateTime<Utc>)> = sqlx::query_as(
            "SELECT data, expires_at FROM web_sessions WHERE id = $1 AND expires_at > now()",
        )
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(|(data, expires_at)| {
            let data: HashMap<String, serde_json::Value> =
                serde_json::from_value(data).map_err(|e| Error::Decode(e.to_string()))?;
            let expiry_date = OffsetDateTime::from_unix_timestamp(expires_at.timestamp())
                .map_err(|e| Error::Decode(e.to_string()))?;
            Ok(Record {
                id: *session_id,
                data,
                expiry_date,
            })
        })
        .transpose()
    }

    async fn delete(&self, session_id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM web_sessions WHERE id = $1")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_sessions::cookie::time::Duration;

    fn record(expires_in: Duration) -> Record {
        Record {
            id: Id::default(),
            data: HashMap::from([("k".to_string(), serde_json::json!("v"))]),
            expiry_date: OffsetDateTime::now_utc() + expires_in,
        }
    }

    // Needs a PostgreSQL container (testcontainers).
    #[tokio::test]
    async fn a_session_survives_a_restart_and_expires() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let store = PgSessionStore::new(pool.clone());
        let mut live = record(Duration::hours(1));
        store.create(&mut live).await.unwrap();

        // A new store on the same database stands in for a restarted server.
        let restarted = PgSessionStore::new(pool.clone());
        let loaded = restarted
            .load(&live.id)
            .await
            .unwrap()
            .expect("session persists");
        assert_eq!(loaded.data["k"], "v");

        let mut updated = loaded;
        updated.data.insert("k".into(), serde_json::json!("w"));
        restarted.save(&updated).await.unwrap();
        assert_eq!(store.load(&live.id).await.unwrap().unwrap().data["k"], "w");

        let expired = record(Duration::seconds(-5));
        store.save(&expired).await.unwrap();
        assert!(store.load(&expired.id).await.unwrap().is_none());
        store.delete(&live.id).await.unwrap();
        assert!(store.load(&live.id).await.unwrap().is_none());
    }
}
