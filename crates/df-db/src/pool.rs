//! PostgreSQL connection pool construction with file-backed passwords and schema `search_path`.

use df_core::{ConfigError, DatabaseConfig};
use secrecy::ExposeSecret;
use sqlx::Executor;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use thiserror::Error;

/// Database layer error type.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    #[error("invalid SQL identifier `{0}`")]
    InvalidIdentifier(String),
    #[error("invalid database data: {0}")]
    InvalidData(String),
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    /// The change violates a uniqueness or immutability rule.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("permission or role management error: {0}")]
    RoleManagement(String),
}

/// Maximum length of a PostgreSQL identifier (`NAMEDATALEN - 1` = 63 bytes).
pub const MAX_SQL_IDENTIFIER_LEN: usize = 63;

/// Validates that `ident` is a safe PostgreSQL identifier (`^[a-zA-Z0-9_-]+$`)
/// up to 63 bytes (`MAX_SQL_IDENTIFIER_LEN`) for double-quoted SQL interpolation
/// and catalog lookups.
pub fn validate_sql_identifier(ident: &str) -> Result<&str, DbError> {
    if ident.is_empty()
        || ident.len() > MAX_SQL_IDENTIFIER_LEN
        || !ident
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(DbError::InvalidIdentifier(ident.to_string()));
    }
    Ok(ident)
}

/// Builds [`PgConnectOptions`] from [`DatabaseConfig`], reading the password from `password_file`.
pub fn build_connect_options(cfg: &DatabaseConfig) -> Result<PgConnectOptions, DbError> {
    validate_sql_identifier(&cfg.schema)?;
    let password = cfg.read_password()?;
    Ok(PgConnectOptions::new()
        .host(&cfg.host)
        .port(cfg.port)
        .database(&cfg.database)
        .username(&cfg.user)
        .password(password.expose_secret()))
}

/// Creates a [`PgPool`] from [`DatabaseConfig`] that sets `search_path` to `cfg.schema` on every connection.
pub async fn connect_pool(cfg: &DatabaseConfig) -> Result<PgPool, DbError> {
    let schema = validate_sql_identifier(&cfg.schema)?.to_string();
    let connect_options = build_connect_options(cfg)?;

    let pool = PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .after_connect(move |conn, _meta| {
            let schema = schema.clone();
            Box::pin(async move {
                let query = format!("SET search_path TO \"{schema}\"");
                // `schema` was validated by `validate_sql_identifier`
                conn.execute(sqlx::AssertSqlSafe(query)).await?;
                Ok(())
            })
        })
        .connect_with(connect_options)
        .await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_core::config::MigrateRoleConfig;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn validates_sql_identifiers() {
        assert!(validate_sql_identifier("dark_factory").is_ok());
        assert!(validate_sql_identifier("dark-factory-dev-app").is_ok());
        assert!(validate_sql_identifier("_schema1").is_ok());
        assert!(validate_sql_identifier("").is_err());
        assert!(validate_sql_identifier("1bad").is_ok());
        assert!(validate_sql_identifier("-bad").is_ok());
        assert!(validate_sql_identifier("bad\"quote").is_err());
        assert!(validate_sql_identifier("bad; DROP TABLE users;--").is_err());
        assert!(validate_sql_identifier("bad name").is_err());
        let exact_63 = "a".repeat(63);
        assert!(validate_sql_identifier(&exact_63).is_ok());
        let too_long_64 = "a".repeat(64);
        assert!(validate_sql_identifier(&too_long_64).is_err());
    }

    #[tokio::test]
    async fn connect_pool_sets_schema_search_path() {
        let server = df_test_postgres::server().await;
        let pool = server.database().await;
        sqlx::query("CREATE SCHEMA IF NOT EXISTS custom_df_schema")
            .execute(&pool)
            .await
            .unwrap();

        let row: (String,) = sqlx::query_as("SELECT current_database()")
            .fetch_one(&pool)
            .await
            .unwrap();
        let db_name = row.0;

        let mut pw_file = NamedTempFile::new().unwrap();
        writeln!(pw_file, "postgres").unwrap();

        let cfg = DatabaseConfig {
            host: server.host().into(),
            port: server.port(),
            database: db_name,
            user: "postgres".into(),
            password_file: pw_file.path().to_path_buf(),
            schema: "custom_df_schema".into(),
            max_connections: 2,
            migrate: MigrateRoleConfig::default(),
        };

        let custom_pool = connect_pool(&cfg).await.unwrap();
        let (current_schema,): (String,) = sqlx::query_as("SELECT current_schema()")
            .fetch_one(&custom_pool)
            .await
            .unwrap();
        assert_eq!(current_schema, "custom_df_schema");
    }
}
