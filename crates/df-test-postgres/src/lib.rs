//! PostgreSQL for tests: one container per machine, one database per test.
//!
//! Use it as a dev-dependency: it pulls in testcontainers, which must not end up in a binary.
//! The migrator is passed in, so this crate does not depend on the crate that owns the migrations.
//!
//! # Container lifecycle
//! testcontainers has no reaper (Ryuk) and does not drop statics at process exit, so a container
//! per process would leak. The container is marked reusable instead: all test binaries on the
//! machine share one container, which keeps running. Remove it with `docker ps -a` /
//! `docker rm -f <name>` (`df-test-postgres-*`). The name carries [`IMAGE_TAG`] and
//! [`CONFIG_REVISION`], because reuse matches by name only: changing the image or its settings
//! starts a new container.
//!
//! Tests never share a database. Databases older than [`STALE_AFTER`] (the creation time is in the
//! UUIDv7 of the name) are dropped when a test binary starts, so databases of tests that are still
//! running in another binary survive. `DROP DATABASE` has no `FORCE`, which protects a test that
//! runs longer than the grace period.
//!
//! # Templates
//! [`migrated_pool`] migrates a template database once per migration set and clones it for every
//! test (`CREATE DATABASE ... TEMPLATE`), which is much faster than migrating each database. The
//! template is named after the versions and checksums of the migrations, so changed migrations get
//! a new template. Old templates stay until the container is removed.
//!
//! When several test binaries start at the same time on a machine without the container, one
//! creates it and the others fail with a name conflict until it is running. They retry for up to
//! [`CONNECT_TIMEOUT`] and then find the container of the winner.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    PgPool,
    migrate::Migrator,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use testcontainers::{
    GenericImage, ImageExt, ReuseDirective,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// Tag of the `postgres` image.
const IMAGE_TAG: &str = "17-alpine";
/// Bump when changing the server settings or environment below, so existing containers are not reused.
const CONFIG_REVISION: u32 = 1;
const USER: &str = "postgres";
const PASSWORD: &str = "postgres";
const DATABASE_PREFIX: &str = "df_test_";
/// Migrated, connection-less databases that test databases are cloned from.
const TEMPLATE_PREFIX: &str = "df_tpl_";
/// A template under construction; renamed to its final name when complete.
const TEMPLATE_TMP_PREFIX: &str = "df_tpl_tmp_";
/// How test databases are cloned from the template: `FILE_COPY` or `WAL_LOG` (PostgreSQL 15+).
const CLONE_STRATEGY: &str = "FILE_COPY";
/// Databases older than this are dropped when a test binary starts.
const STALE_AFTER: Duration = Duration::from_secs(15 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const START_RETRY_PAUSE: Duration = Duration::from_millis(500);

static SERVER: OnceCell<TestServer> = OnceCell::const_new();

/// Address of the shared PostgreSQL server; the `postgres` role is a superuser.
#[derive(Debug)]
pub struct TestServer {
    host: String,
    port: u16,
}

/// The shared server, started on first use.
///
/// # Panics
/// When Docker is unavailable or PostgreSQL does not accept connections in time.
pub async fn server() -> &'static TestServer {
    SERVER
        .get_or_init(|| async {
            TestServer::start()
                .await
                .expect("start PostgreSQL container")
        })
        .await
}

/// A pool on a new, empty database.
pub async fn empty_pool() -> PgPool {
    server().await.database().await
}

/// A pool on a new database with `migrator` applied.
pub async fn migrated_pool(migrator: &Migrator) -> PgPool {
    let server = server().await;
    let template = server.template(migrator).await;
    server.database_from(Some(&template)).await
}

/// Name of the template database for this migration set.
fn template_name(migrator: &Migrator) -> String {
    // The hash only has to be stable within one toolchain: a new toolchain builds a new template.
    let mut hasher = DefaultHasher::new();
    for migration in migrator.iter() {
        migration.version.hash(&mut hasher);
        migration.checksum.hash(&mut hasher);
    }
    format!("{TEMPLATE_PREFIX}{:016x}", hasher.finish())
}

/// Calls `attempt` until it succeeds or `timeout` has passed, pausing `pause` in between. Returns
/// the first error if it never succeeds (a later one is usually a consequence of it).
async fn retry_for<T, E: std::fmt::Display, F: Future<Output = Result<T, E>>>(
    timeout: Duration,
    pause: Duration,
    mut attempt: impl FnMut() -> F,
) -> Result<T, E> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut first_error = None;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                // No tracing subscriber in tests.
                eprintln!("starting the PostgreSQL container failed, retrying: {e}");
                first_error.get_or_insert(e);
                if let Some(first) =
                    first_error.take_if(|_| tokio::time::Instant::now() >= deadline)
                {
                    return Err(first);
                }
                tokio::time::sleep(pause).await;
            }
        }
    }
}

impl TestServer {
    /// Starts (or finds) the container; retries because of the start race described in the module docs.
    async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let server = retry_for(CONNECT_TIMEOUT, START_RETRY_PAUSE, Self::start_container).await?;
        server.wait_until_ready().await?;
        drop_stale_databases(&server.admin_pool(1).await).await;
        Ok(server)
    }

    async fn start_container() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let container = GenericImage::new("postgres", IMAGE_TAG)
            .with_exposed_port(ContainerPort::Tcp(5432))
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_PASSWORD", PASSWORD)
            .with_cmd(["postgres", "-c", "fsync=off", "-c", "max_connections=500"])
            .with_container_name(format!("df-test-postgres-{IMAGE_TAG}-r{CONFIG_REVISION}"))
            .with_reuse(ReuseDirective::Always)
            .start()
            .await?;
        // Dropping the handle leaves a reusable container running.
        let host = match container.get_host().await?.to_string().as_str() {
            // `localhost` may resolve to `::1`, which VMs without IPv6 cannot reach.
            "localhost" => "127.0.0.1".to_owned(),
            host => host.to_owned(),
        };
        Ok(Self {
            host,
            port: container.get_host_port_ipv4(5432).await?,
        })
    }

    /// The image logs the "ready" message once for its temporary init server before the real one
    /// listens, so wait for an actual connection.
    async fn wait_until_ready(&self) -> Result<(), sqlx::Error> {
        let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
        loop {
            match self.pool("postgres", 1).await {
                Ok(_) => return Ok(()),
                Err(e) if tokio::time::Instant::now() >= deadline => return Err(e),
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// A pool on the `postgres` database as the superuser, for role and database setup.
    pub async fn admin_pool(&self, max_connections: u32) -> PgPool {
        self.pool("postgres", max_connections)
            .await
            .expect("connect to the test server")
    }

    /// A pool on a new, empty database.
    pub async fn database(&self) -> PgPool {
        self.database_from(None).await
    }

    /// A pool on a new database, cloned from `template` if given.
    ///
    /// Pools are created per call and never cached in a static: every `#[tokio::test]` has its own
    /// runtime, and a pool must not outlive the runtime it was created on.
    async fn database_from(&self, template: Option<&str>) -> PgPool {
        let name = format!("{DATABASE_PREFIX}{}", Uuid::now_v7().simple());
        let template = template.map_or_else(String::new, |t| {
            format!(" TEMPLATE \"{t}\" STRATEGY {CLONE_STRATEGY}")
        });
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE \"{name}\"{template}"
        )))
        .execute(&self.admin_pool(1).await)
        .await
        .expect("create the test database");
        self.pool(&name, 5)
            .await
            .expect("connect to the test database")
    }

    /// Name of the migrated template for `migrator`, built if it does not exist yet.
    ///
    /// Safe across processes: the template is built under a temporary name and renamed when done
    /// (atomic), so nobody clones a half-migrated database; the loser of a race drops its copy.
    async fn template(&self, migrator: &Migrator) -> String {
        let name = template_name(migrator);
        let admin = self.admin_pool(1).await;
        if database_exists(&admin, &name).await {
            return name;
        }

        let tmp = format!("{TEMPLATE_TMP_PREFIX}{}", Uuid::now_v7().simple());
        create_database(&admin, &tmp).await;
        let pool = self
            .pool(&tmp, 1)
            .await
            .expect("connect to the template database");
        migrator
            .run(&pool)
            .await
            .expect("apply migrations to the template database");
        pool.close().await;
        execute(
            &admin,
            &format!("ALTER DATABASE \"{tmp}\" WITH ALLOW_CONNECTIONS false IS_TEMPLATE true"),
        )
        .await
        .expect("mark the database as a template");

        // The closed sessions can take a moment to disappear; a taken name means another process won.
        for _ in 0..50 {
            match execute(
                &admin,
                &format!("ALTER DATABASE \"{tmp}\" RENAME TO \"{name}\""),
            )
            .await
            {
                Ok(()) => return name,
                Err(_) if database_exists(&admin, &name).await => break,
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        // Lost the race, or gave up: the template must exist either way.
        let _ = execute(
            &admin,
            &format!("ALTER DATABASE \"{tmp}\" IS_TEMPLATE false"),
        )
        .await;
        let _ = execute(&admin, &format!("DROP DATABASE \"{tmp}\"")).await;
        assert!(
            database_exists(&admin, &name).await,
            "could not create the template database {name}"
        );
        name
    }

    async fn pool(&self, database: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
        let options = PgConnectOptions::new()
            .host(&self.host)
            .port(self.port)
            .username(USER)
            .password(PASSWORD)
            .database(database);
        PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
    }
}

async fn execute(admin: &PgPool, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(admin)
        .await
        .map(|_| ())
}

async fn create_database(admin: &PgPool, name: &str) {
    execute(admin, &format!("CREATE DATABASE \"{name}\""))
        .await
        .expect("create the database");
}

async fn database_exists(admin: &PgPool, name: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT FROM pg_database WHERE datname = $1)")
        .bind(name)
        .fetch_one(admin)
        .await
        .expect("query pg_database")
}

/// When the database `name` (`<prefix><UUIDv7>`) was created, if it follows that scheme.
fn created_at(name: &str, prefix: &str) -> Option<SystemTime> {
    let (secs, nanos) = Uuid::parse_str(name.strip_prefix(prefix)?)
        .ok()?
        .get_timestamp()?
        .to_unix();
    Some(UNIX_EPOCH + Duration::new(secs, nanos))
}

/// Drops test databases and unfinished templates older than [`STALE_AFTER`]. Names that do not
/// follow the scheme are left alone; all errors are ignored (best effort).
async fn drop_stale_databases(admin: &PgPool) {
    let names: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database")
        .fetch_all(admin)
        .await
        .unwrap_or_default();
    let cutoff = SystemTime::now() - STALE_AFTER;
    for name in names {
        let is_stale = |prefix| created_at(&name, prefix).is_some_and(|created| created < cutoff);
        if is_stale(TEMPLATE_TMP_PREFIX) {
            // A build that died after marking the database as template; templates cannot be dropped.
            let _ = execute(
                admin,
                &format!("ALTER DATABASE \"{name}\" IS_TEMPLATE false"),
            )
            .await;
        } else if !is_stale(DATABASE_PREFIX) {
            continue;
        }
        let _ = execute(admin, &format!("DROP DATABASE \"{name}\"")).await;
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use sqlx::{
        SqlSafeStr,
        migrate::{Migration, MigrationType},
    };

    use super::*;

    static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

    async fn table_exists(pool: &PgPool, name: &str) -> sqlx::Result<bool> {
        sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(name)
            .fetch_one(pool)
            .await
    }

    #[tokio::test]
    async fn retry_for_succeeds_after_failures_and_keeps_the_first_error_when_giving_up()
    -> Result<(), String> {
        let mut calls = 0;
        let value = retry_for(Duration::from_secs(5), Duration::from_millis(1), || {
            calls += 1;
            let n = calls;
            async move {
                if n < 3 {
                    Err(format!("error {n}"))
                } else {
                    Ok(n)
                }
            }
        })
        .await?;
        assert_eq!(value, 3);

        let err = retry_for(Duration::from_millis(20), Duration::from_millis(5), || {
            calls += 1;
            let n = calls;
            async move { Err::<(), _>(format!("error {n}")) }
        })
        .await
        .unwrap_err();
        assert_eq!(err, "error 4");
        Ok(())
    }

    #[tokio::test]
    async fn pools_are_isolated() -> sqlx::Result<()> {
        let (a, b) = (empty_pool().await, empty_pool().await);
        sqlx::query("CREATE TABLE only_in_a (id int)")
            .execute(&a)
            .await?;
        assert!(table_exists(&a, "only_in_a").await?);
        assert!(!table_exists(&b, "only_in_a").await?);
        Ok(())
    }

    #[tokio::test]
    async fn migrated_pool_applies_the_migrator_and_empty_pool_does_not() -> sqlx::Result<()> {
        let migrated = migrated_pool(&MIGRATOR).await;
        let empty = empty_pool().await;
        assert!(table_exists(&migrated, "migrated").await?);
        assert!(!table_exists(&empty, "migrated").await?);
        Ok(())
    }

    #[tokio::test]
    async fn server_connects_as_superuser() -> sqlx::Result<()> {
        let server = server().await;
        assert_ne!(server.port(), 0);
        let superuser: bool =
            sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname = current_user")
                .fetch_one(&server.admin_pool(1).await)
                .await?;
        assert!(superuser);
        Ok(())
    }

    /// Name of a database created `age` ago, as `drop_stale_databases` reads it from the UUIDv7.
    fn aged_name(prefix: &str, age: Duration) -> String {
        let at = SystemTime::now() - age;
        let since_epoch = at.duration_since(UNIX_EPOCH).expect("after the epoch");
        let ts = uuid::Timestamp::from_unix(
            uuid::NoContext,
            since_epoch.as_secs(),
            since_epoch.subsec_nanos(),
        );
        format!("{prefix}{}", Uuid::new_v7(ts).simple())
    }

    #[tokio::test]
    async fn only_databases_older_than_the_grace_period_are_dropped() -> sqlx::Result<()> {
        let admin = server().await.admin_pool(1).await;
        let old = aged_name(DATABASE_PREFIX, STALE_AFTER + Duration::from_secs(60));
        let fresh = aged_name(DATABASE_PREFIX, Duration::ZERO);
        let old_tmp = aged_name(TEMPLATE_TMP_PREFIX, STALE_AFTER + Duration::from_secs(60));
        let fresh_tmp = aged_name(TEMPLATE_TMP_PREFIX, Duration::ZERO);
        let unparsable = format!("{DATABASE_PREFIX}not_a_uuid_{}", Uuid::now_v7().simple());
        migrated_pool(&MIGRATOR).await;
        let template = template_name(&MIGRATOR);
        for name in [&old, &fresh, &old_tmp, &fresh_tmp, &unparsable] {
            create_database(&admin, name).await;
        }
        // A build that died between marking the database as template and renaming it.
        execute(
            &admin,
            &format!("ALTER DATABASE \"{old_tmp}\" WITH IS_TEMPLATE true"),
        )
        .await?;

        drop_stale_databases(&admin).await;

        assert!(!database_exists(&admin, &old).await);
        assert!(!database_exists(&admin, &old_tmp).await);
        for kept in [&fresh, &fresh_tmp, &unparsable, &template] {
            assert!(database_exists(&admin, kept).await, "{kept} was dropped");
        }
        Ok(())
    }

    #[tokio::test]
    async fn template_is_built_once_and_cannot_be_connected_to() -> sqlx::Result<()> {
        let admin = server().await.admin_pool(1).await;
        let template = template_name(&MIGRATOR);
        let state = || async {
            sqlx::query_as::<_, (i32, bool, bool)>(
                "SELECT oid::int, datallowconn, datistemplate FROM pg_database WHERE datname = $1",
            )
            .bind(&template)
            .fetch_one(&admin)
            .await
        };

        migrated_pool(&MIGRATOR).await;
        let (oid, allow_connections, is_template) = state().await?;
        migrated_pool(&MIGRATOR).await;

        assert!(!allow_connections);
        assert!(is_template);
        assert_eq!(state().await?.0, oid, "the template was rebuilt");
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_first_use_builds_exactly_one_template() -> sqlx::Result<()> {
        // A migration set no other test uses, so its template does not exist yet.
        let version = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("after the epoch")
                .as_micros(),
        )
        .expect("fits in i64");
        let migrator = std::sync::Arc::new(Migrator {
            migrations: Cow::Owned(vec![Migration::new(
                version,
                "unique".into(),
                MigrationType::Simple,
                "CREATE TABLE unique_table (id int)".into_sql_str(),
                false,
            )]),
            ..Migrator::DEFAULT
        });

        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let migrator = migrator.clone();
            tasks.spawn(async move { migrated_pool(&migrator).await });
        }
        let mut pools = Vec::new();
        while let Some(pool) = tasks.join_next().await {
            pools.push(pool.expect("task panicked"));
        }

        for pool in &pools {
            assert!(table_exists(pool, "unique_table").await?);
        }
        let admin = server().await.admin_pool(1).await;
        let templates: i64 =
            sqlx::query_scalar("SELECT count(*) FROM pg_database WHERE datname = $1")
                .bind(template_name(&migrator))
                .fetch_one(&admin)
                .await?;
        assert_eq!(templates, 1);
        Ok(())
    }
}
