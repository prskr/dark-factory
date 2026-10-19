//! `dark-factory migrate` implementation: embedded migrations, optional application-role
//! management, and least-privilege grants (`design/deployment.md` §4.2,
//! `specs/database-migrations/spec.md`).

use df_core::DatabaseConfig;
use secrecy::ExposeSecret;
use sqlx::{ConnectOptions, Connection, Executor, PgConnection};
use tracing::info;

use crate::MIGRATOR;
use crate::pool::{DbError, build_connect_options, connect_pool, validate_sql_identifier};

/// Advisory lock key used to serialize whole `dark-factory migrate` runs (schema creation,
/// embedded migrations, role management, and `GRANT`/`REVOKE` catalog updates).
const MIGRATE_LOCK_NAMESPACE: i32 = 0x4446_5f4d; // "DF_M"

fn advisory_lock_subkey(schema: &str) -> i32 {
    let mut hash: u32 = 2_166_136_261;
    for b in schema.as_bytes() {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash as i32
}

/// Validates and quotes a PostgreSQL identifier (such as a database name that may contain
/// hyphens) as `"..."` with embedded double quotes doubled (`""`). Rejects empty strings,
/// identifiers longer than 63 bytes (`MAX_SQL_IDENTIFIER_LEN`), and NUL bytes.
pub fn quote_sql_identifier(ident: &str) -> Result<String, DbError> {
    if ident.is_empty() || ident.len() > crate::pool::MAX_SQL_IDENTIFIER_LEN || ident.contains('\0')
    {
        return Err(DbError::InvalidIdentifier(ident.to_string()));
    }
    Ok(format!("\"{}\"", ident.replace('"', "\"\"")))
}

/// Runs embedded migrations, optionally creates/rotates the application role,
/// and applies least-privilege grants in the configured schema.
pub async fn run_migrate(cfg: &DatabaseConfig) -> Result<(), DbError> {
    let quoted_db_name = quote_sql_identifier(&cfg.database)?;
    let schema = validate_sql_identifier(&cfg.schema)?;
    let app_role = validate_sql_identifier(&cfg.migrate.app_role)?;

    // Open a dedicated connection (outside the pool) for the session-level advisory lock
    // so `max_connections = 1` on the pool never deadlocks waiting for a free pool slot.
    let lock_opts = build_connect_options(cfg)?;
    let mut lock_conn = PgConnection::connect_with(&lock_opts).await?;
    let subkey = advisory_lock_subkey(schema);
    sqlx::query("SELECT pg_advisory_lock($1, $2)")
        .bind(MIGRATE_LOCK_NAMESPACE)
        .bind(subkey)
        .execute(&mut lock_conn)
        .await?;

    let pool = connect_pool(cfg).await?;
    let res = run_migrate_locked(cfg, &pool, &quoted_db_name, schema, app_role).await;
    pool.close().await;

    let _ = sqlx::query("SELECT pg_advisory_unlock($1, $2)")
        .bind(MIGRATE_LOCK_NAMESPACE)
        .bind(subkey)
        .execute(&mut lock_conn)
        .await;
    let _ = lock_conn.close().await;

    res
}

async fn run_migrate_locked(
    cfg: &DatabaseConfig,
    pool: &sqlx::PgPool,
    quoted_db_name: &str,
    schema: &str,
    app_role: &str,
) -> Result<(), DbError> {
    // Ensure the target schema exists when connected as owner.
    let create_schema_sql = format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\"");
    // `schema` was validated by `validate_sql_identifier`
    pool.execute(sqlx::AssertSqlSafe(create_schema_sql)).await?;

    // Step 1: Apply embedded migrations under sqlx's advisory lock and checksum verification.
    MIGRATOR.run(pool).await?;
    info!(schema = %schema, "applied embedded database migrations");

    // Step 2: Optional application-role creation / password rotation.
    // Executed on a dedicated connection with statement logging disabled so passwords
    // never appear in logs or spans.
    if cfg.migrate.manage_app_role {
        if let Some(app_password) = cfg.migrate.read_app_password()? {
            manage_application_role(cfg, app_role, app_password.expose_secret()).await?;
        } else {
            return Err(DbError::RoleManagement(
                "migrate.manageAppRole is enabled, but migrate.appPasswordFile is not set".into(),
            ));
        }
    }

    // Step 3: Re-apply grants and audit_log revocations on every run if the app role exists.
    let role_exists: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(app_role)
            .fetch_one(pool)
            .await?;

    if role_exists.0 {
        apply_grants(pool, quoted_db_name, schema, app_role).await?;
        info!(app_role = %app_role, schema = %schema, "applied application role grants and audit_log revocations");
    }

    Ok(())
}

async fn manage_application_role(
    cfg: &DatabaseConfig,
    app_role: &str,
    password: &str,
) -> Result<(), DbError> {
    let opts = build_connect_options(cfg)?.disable_statement_logging();
    let mut conn = PgConnection::connect_with(&opts).await?;

    let (exists,): (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(app_role)
            .fetch_one(&mut conn)
            .await?;

    let escaped_pw = password.replace('\'', "''");
    let ddl = if exists {
        format!("ALTER ROLE \"{app_role}\" WITH LOGIN PASSWORD '{escaped_pw}'")
    } else {
        format!("CREATE ROLE \"{app_role}\" LOGIN PASSWORD '{escaped_pw}'")
    };

    // `app_role` was validated by `validate_sql_identifier`, the password quotes are doubled
    conn.execute(sqlx::AssertSqlSafe(ddl))
        .await
        .map_err(|err| {
            DbError::RoleManagement(format!(
                "failed to manage application role `{app_role}` ({err}); \
             if the role is managed externally (e.g. by a DBA), disable role management \
             by setting `database.migrate.manageAppRole: false`"
            ))
        })?;

    if exists {
        info!(app_role = %app_role, "updated application role password");
    } else {
        info!(app_role = %app_role, "created application role");
    }

    Ok(())
}

async fn apply_grants(
    pool: &sqlx::PgPool,
    quoted_db_name: &str,
    schema: &str,
    app_role: &str,
) -> Result<(), DbError> {
    let statements = [
        format!("GRANT CONNECT ON DATABASE {quoted_db_name} TO \"{app_role}\""),
        format!("GRANT USAGE ON SCHEMA \"{schema}\" TO \"{app_role}\""),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA \"{schema}\" TO \"{app_role}\""
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA \"{schema}\" TO \"{app_role}\""),
        format!("REVOKE UPDATE, DELETE, TRUNCATE ON \"{schema}\".audit_log FROM \"{app_role}\""),
    ];

    let mut tx = pool.begin().await?;
    for stmt in statements {
        // Identifiers were validated/quoted by the caller
        tx.execute(sqlx::AssertSqlSafe(stmt)).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{NewAuditEntry, write_audit_entry};
    use df_core::Principal;
    use df_core::config::MigrateRoleConfig;
    use sqlx::PgPool;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    fn write_secret_temp(secret: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{secret}").unwrap();
        f
    }

    #[test]
    fn quote_sql_identifier_accepts_hyphenated_database_names() {
        assert_eq!(
            quote_sql_identifier("dark-factory").unwrap(),
            "\"dark-factory\""
        );
        assert!(quote_sql_identifier("").is_err());
        assert!(quote_sql_identifier("bad\0name").is_err());
        let exact_63 = "a".repeat(63);
        assert!(quote_sql_identifier(&exact_63).is_ok());
        let too_long_64 = "a".repeat(64);
        assert!(quote_sql_identifier(&too_long_64).is_err());
    }

    async fn setup_owner_and_schema(
        superuser_pool: &PgPool,
        suffix: &str,
    ) -> (String, String, String, String) {
        let (db_name,): (String,) = sqlx::query_as("SELECT current_database()")
            .fetch_one(superuser_pool)
            .await
            .unwrap();
        let owner_role = format!("df_owner_{suffix}");
        let app_role = format!("df_app_{suffix}");
        let schema = format!("df_schema_{suffix}");

        // Clean up any leftover roles from previous runs
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP ROLE IF EXISTS \"{app_role}\""
        )))
        .execute(superuser_pool)
        .await;
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP ROLE IF EXISTS \"{owner_role}\""
        )))
        .execute(superuser_pool)
        .await;

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE \"{owner_role}\" LOGIN PASSWORD 'owner-pass' CREATEROLE"
        )))
        .execute(superuser_pool)
        .await
        .unwrap();

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "GRANT ALL PRIVILEGES ON DATABASE \"{db_name}\" TO \"{owner_role}\" WITH GRANT OPTION"
        )))
        .execute(superuser_pool)
        .await
        .unwrap();

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE SCHEMA \"{schema}\" AUTHORIZATION \"{owner_role}\""
        )))
        .execute(superuser_pool)
        .await
        .unwrap();

        (db_name, owner_role, app_role, schema)
    }

    async fn cleanup_roles(
        superuser_pool: &PgPool,
        schema: &str,
        owner_role: &str,
        app_role: &str,
    ) {
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"
        )))
        .execute(superuser_pool)
        .await;
        let (db_name,): (String,) = sqlx::query_as("SELECT current_database()")
            .fetch_one(superuser_pool)
            .await
            .unwrap();
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "REVOKE ALL PRIVILEGES ON DATABASE \"{db_name}\" FROM \"{app_role}\""
        )))
        .execute(superuser_pool)
        .await;
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "REVOKE ALL PRIVILEGES ON DATABASE \"{db_name}\" FROM \"{owner_role}\""
        )))
        .execute(superuser_pool)
        .await;
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP ROLE IF EXISTS \"{app_role}\""
        )))
        .execute(superuser_pool)
        .await;
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP ROLE IF EXISTS \"{owner_role}\""
        )))
        .execute(superuser_pool)
        .await;
    }

    #[sqlx::test(migrations = false)]
    async fn migrate_fresh_schema_role_creation_password_rotation_and_grants(
        superuser_pool: PgPool,
    ) {
        let suffix = Uuid::now_v7().simple().to_string();
        let (db_name, owner_role, app_role, schema) =
            setup_owner_and_schema(&superuser_pool, &suffix).await;

        let owner_pw = write_secret_temp("owner-pass");
        let app_pw_v1 = write_secret_temp("app-pass-v1");

        // Use max_connections = 1 to verify advisory locking does not deadlock a single-connection pool
        let cfg = DatabaseConfig {
            host: "127.0.0.1".into(),
            port: 5432,
            database: db_name.clone(),
            user: owner_role.clone(),
            password_file: owner_pw.path().to_path_buf(),
            schema: schema.clone(),
            max_connections: 1,
            migrate: MigrateRoleConfig {
                app_role: app_role.clone(),
                manage_app_role: true,
                app_password_file: Some(app_pw_v1.path().to_path_buf()),
            },
        };

        // Scenario: Fresh database + Role missing -> creates role and applies migrations and grants
        run_migrate(&cfg)
            .await
            .expect("initial migrate should succeed");

        // Connect as the newly created application role with password v1
        let app_cfg_v1 = DatabaseConfig {
            host: "127.0.0.1".into(),
            port: 5432,
            database: db_name.clone(),
            user: app_role.clone(),
            password_file: app_pw_v1.path().to_path_buf(),
            schema: schema.clone(),
            max_connections: 2,
            migrate: MigrateRoleConfig::default(),
        };
        let app_pool = connect_pool(&app_cfg_v1)
            .await
            .expect("app role should connect with v1 password");

        // App role can INSERT and SELECT on audit_log
        let entry = NewAuditEntry::new(
            Principal::system("test"),
            Vec::<String>::new(),
            "test.action",
            "test",
            "1",
        );
        let inserted = write_audit_entry(&app_pool, &entry)
            .await
            .expect("app role can INSERT into audit_log");

        // Scenario: Application role cannot rewrite audit history (UPDATE or DELETE rejected)
        let update_audit = sqlx::query("UPDATE audit_log SET action = 'tampered' WHERE id = $1")
            .bind(inserted.id)
            .execute(&app_pool)
            .await;
        assert!(
            update_audit.is_err(),
            "app role must not be permitted to UPDATE audit_log"
        );

        let delete_audit = sqlx::query("DELETE FROM audit_log WHERE id = $1")
            .bind(inserted.id)
            .execute(&app_pool)
            .await;
        assert!(
            delete_audit.is_err(),
            "app role must not be permitted to DELETE from audit_log"
        );

        // Scenario: Application role has no DDL rights (CREATE TABLE rejected)
        let create_table = sqlx::query("CREATE TABLE unauthorized_ddl (id INT)")
            .execute(&app_pool)
            .await;
        assert!(
            create_table.is_err(),
            "app role must not have CREATE TABLE rights in schema"
        );
        app_pool.close().await;

        // Scenario: Password rotation
        let app_pw_v2 = write_secret_temp("app-pass-v2");
        let mut cfg_v2 = cfg.clone();
        cfg_v2.migrate.app_password_file = Some(app_pw_v2.path().to_path_buf());
        run_migrate(&cfg_v2)
            .await
            .expect("password rotation migrate should succeed");

        let mut app_cfg_v2 = app_cfg_v1;
        app_cfg_v2.password_file = app_pw_v2.path().to_path_buf();
        let rotated_pool = connect_pool(&app_cfg_v2)
            .await
            .expect("app role should connect with rotated v2 password");
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log")
            .fetch_one(&rotated_pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        rotated_pool.close().await;

        cleanup_roles(&superuser_pool, &schema, &owner_role, &app_role).await;
    }

    #[sqlx::test(migrations = false)]
    async fn concurrent_migrate_runs_and_tampered_checksum_and_unmanageable_role(
        superuser_pool: PgPool,
    ) {
        let suffix = Uuid::now_v7().simple().to_string();
        let (db_name, owner_role, app_role, schema) =
            setup_owner_and_schema(&superuser_pool, &suffix).await;

        // Pre-create `app_role` as superuser so `owner_role` (without ADMIN option on `app_role`)
        // cannot ALTER it.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE \"{app_role}\" LOGIN PASSWORD 'dba-created'"
        )))
        .execute(&superuser_pool)
        .await
        .unwrap();

        let owner_pw = write_secret_temp("owner-pass");
        let app_pw = write_secret_temp("new-pass");

        // Scenario: Role not manageable -> exits non-zero with message to disable role management
        let cfg_manage = DatabaseConfig {
            host: "127.0.0.1".into(),
            port: 5432,
            database: db_name.clone(),
            user: owner_role.clone(),
            password_file: owner_pw.path().to_path_buf(),
            schema: schema.clone(),
            max_connections: 2,
            migrate: MigrateRoleConfig {
                app_role: app_role.clone(),
                manage_app_role: true,
                app_password_file: Some(app_pw.path().to_path_buf()),
            },
        };
        let err = run_migrate(&cfg_manage).await.unwrap_err();
        assert!(
            err.to_string().contains("manageAppRole: false"),
            "expected error instructing to disable role management, got: {err}"
        );

        // Scenario: Concurrent runs with role management disabled -> both succeed
        let mut cfg_no_manage = cfg_manage.clone();
        cfg_no_manage.migrate.manage_app_role = false;
        let c1 = cfg_no_manage.clone();
        let c2 = cfg_no_manage.clone();
        let (r1, r2) = tokio::join!(run_migrate(&c1), run_migrate(&c2));
        r1.expect("concurrent migrate 1 should succeed");
        r2.expect("concurrent migrate 2 should succeed");

        // Scenario: Tampered migration checksum -> fails naming the migration
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE \"{schema}\"._sqlx_migrations SET checksum = decode('00', 'hex') WHERE version = 20261017000001"
        )))
        .execute(&superuser_pool)
        .await
        .unwrap();

        let tampered_err = run_migrate(&cfg_no_manage).await.unwrap_err();
        assert!(
            tampered_err.to_string().contains("20261017000001"),
            "expected tampered checksum error to name migration 20261017000001, got: {tampered_err}"
        );

        cleanup_roles(&superuser_pool, &schema, &owner_role, &app_role).await;
    }
}
