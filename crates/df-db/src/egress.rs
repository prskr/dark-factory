//! Egress allowlist persistence operations.
//!
//! Changes to entries write an audit entry and publish `egress_policy` in the transaction that
//! changes them, so the notification reaches the gateways only for committed changes.

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::Actor;
use crate::notify::{Notification, publish};
use crate::pool::DbError;

/// What an allowlist entry allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EntryKind {
    /// Outbound HTTP(S) to a hostname (or `*.suffix`) and port through the gateway proxy.
    HttpHost { host: String, port: u16 },
    /// Direct access to an in-cluster Service.
    ClusterService {
        namespace: String,
        service: String,
        ports: Vec<u16>,
        allow_dns: bool,
    },
}

/// A stored allowlist entry; `project_key` is `None` for instance scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryRecord {
    pub id: Uuid,
    pub project_key: Option<String>,
    #[serde(flatten)]
    pub kind: EntryKind,
}

struct EntryRow {
    id: Uuid,
    project_key: Option<String>,
    kind: String,
    host: Option<String>,
    port: Option<i32>,
    namespace: Option<String>,
    service: Option<String>,
    ports: Vec<i32>,
    allow_dns: bool,
}

impl TryFrom<EntryRow> for EntryRecord {
    type Error = DbError;

    fn try_from(row: EntryRow) -> Result<Self, DbError> {
        let corrupt = |what: &str| DbError::InvalidData(format!("egress entry {}: {what}", row.id));
        let kind = match row.kind.as_str() {
            "http-host" => EntryKind::HttpHost {
                host: row.host.clone().ok_or_else(|| corrupt("no host"))?,
                port: row.port.ok_or_else(|| corrupt("no port")).and_then(|p| {
                    port_from_db(p).map_err(|_out_of_range| corrupt("invalid port"))
                })?,
            },
            "cluster-service" => EntryKind::ClusterService {
                namespace: row
                    .namespace
                    .clone()
                    .ok_or_else(|| corrupt("no namespace"))?,
                service: row.service.clone().ok_or_else(|| corrupt("no service"))?,
                ports: row
                    .ports
                    .iter()
                    .map(|p| port_from_db(*p).map_err(|_out_of_range| corrupt("invalid port")))
                    .collect::<Result<_, _>>()?,
                allow_dns: row.allow_dns,
            },
            other => return Err(corrupt(&format!("unknown kind `{other}`"))),
        };
        Ok(Self {
            id: row.id,
            project_key: row.project_key,
            kind,
        })
    }
}

/// An active run with a sandbox address, as the policy snapshot needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRun {
    pub run_id: Uuid,
    pub pod_ip: String,
    pub task_key: String,
    pub run_number: i64,
    pub project_id: Uuid,
    pub project_key: String,
    pub persona_id: Uuid,
}

/// An `http-host` entry for the policy snapshot; `project_id` is `None` for instance scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    pub id: Uuid,
    pub project_id: Option<Uuid>,
    pub host: String,
    pub port: u16,
}

/// The next version of the egress policy, from a sequence shared by all control plane replicas.
pub async fn next_policy_version(pool: &PgPool) -> Result<u64, DbError> {
    let version =
        sqlx::query_scalar!(r#"SELECT nextval('egress_policy_version_seq') AS "version!""#)
            .fetch_one(pool)
            .await?;
    u64::try_from(version).map_err(|_negative| {
        DbError::InvalidData(format!("egress policy version {version} is negative"))
    })
}

/// Runs in a non-terminal state that have a pod address, oldest first.
pub async fn active_runs(pool: &PgPool) -> Result<Vec<ActiveRun>, DbError> {
    Ok(sqlx::query_as!(
        ActiveRun,
        r#"SELECT r.id AS run_id, r.pod_ip AS "pod_ip!", t.key AS task_key, r.number AS run_number, p.id AS project_id,
                  p.key AS project_key, t.persona_id
           FROM runs r
           JOIN tasks t ON t.id = r.task_id
           JOIN projects p ON p.id = t.project_id
           WHERE r.pod_ip IS NOT NULL
             AND r.state NOT IN ('succeeded', 'failed', 'cancelled')
           ORDER BY r.started_at, r.id"#
    )
    .fetch_all(pool)
    .await?)
}

/// All `http-host` entries. An entry with a stored port outside `1..=65535` is skipped (and
/// logged), so a corrupt row narrows the policy instead of widening it.
pub async fn http_host_entries(pool: &PgPool) -> Result<Vec<HostEntry>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT id, project_id, host AS "host!", port AS "port!" FROM egress_allowlist
           WHERE kind = 'http-host' AND host IS NOT NULL AND port IS NOT NULL
           ORDER BY created_at, id"#
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match port_from_db(row.port) {
            Ok(port) => Some(HostEntry {
                id: row.id,
                project_id: row.project_id,
                host: row.host,
                port,
            }),
            Err(error) => {
                tracing::warn!(entry = %row.id, %error, "skipping egress entry with an invalid port");
                None
            }
        })
        .collect())
}

/// All entries, instance scope first, then by project.
pub async fn list_entries(pool: &PgPool) -> Result<Vec<EntryRecord>, DbError> {
    let rows = sqlx::query_as!(
        EntryRow,
        r#"SELECT e.id, p.key AS "project_key?", e.kind, e.host, e.port, e.namespace, e.service,
                  e.ports, e.allow_dns
           FROM egress_allowlist e LEFT JOIN projects p ON p.id = e.project_id
           ORDER BY (e.project_id IS NOT NULL), p.key, e.kind, e.host, e.namespace, e.service,
                    e.id"#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

/// Adds an entry at instance scope (`project_key` is `None`) or for a project. An identical
/// entry is a [`DbError::Conflict`], an unknown project [`DbError::NotFound`].
pub async fn add_entry(
    pool: &PgPool,
    actor: Actor<'_>,
    project_key: Option<&str>,
    kind: &EntryKind,
) -> Result<EntryRecord, DbError> {
    let mut tx = pool.begin().await?;
    let project_id: Option<Uuid> = match project_key {
        Some(key) => Some(
            sqlx::query_scalar!("SELECT id FROM projects WHERE key = $1", key)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError::NotFound(format!("project `{key}`")))?,
        ),
        None => None,
    };
    let duplicate: bool = match kind {
        EntryKind::HttpHost { host, port } => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM egress_allowlist WHERE kind = 'http-host'
                     AND host = $1 AND port = $2 AND project_id IS NOT DISTINCT FROM $3) AS "exists!""#,
                host,
                i32::from(*port),
                project_id
            )
            .fetch_one(&mut *tx)
            .await?
        }
        EntryKind::ClusterService {
            namespace, service, ..
        } => {
            sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM egress_allowlist WHERE kind = 'cluster-service'
                     AND namespace = $1 AND service = $2 AND project_id IS NOT DISTINCT FROM $3) AS "exists!""#,
                namespace,
                service,
                project_id
            )
            .fetch_one(&mut *tx)
            .await?
        }
    };
    if duplicate {
        return Err(DbError::Conflict("this entry already exists".into()));
    }

    let id = Uuid::now_v7();
    let scope = if project_id.is_some() {
        "project"
    } else {
        "instance"
    };
    match kind {
        EntryKind::HttpHost { host, port } => {
            sqlx::query!(
                "INSERT INTO egress_allowlist (id, scope, project_id, kind, host, port)
                 VALUES ($1, $2, $3, 'http-host', $4, $5)",
                id,
                scope,
                project_id,
                host,
                i32::from(*port)
            )
            .execute(&mut *tx)
            .await?;
        }
        EntryKind::ClusterService {
            namespace,
            service,
            ports,
            allow_dns,
        } => {
            let ports: Vec<i32> = ports.iter().map(|p| i32::from(*p)).collect();
            sqlx::query!(
                "INSERT INTO egress_allowlist
                     (id, scope, project_id, kind, namespace, service, ports, allow_dns)
                 VALUES ($1, $2, $3, 'cluster-service', $4, $5, $6, $7)",
                id,
                scope,
                project_id,
                namespace,
                service,
                &ports,
                *allow_dns
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    let record = EntryRecord {
        id,
        project_key: project_key.map(str::to_string),
        kind: kind.clone(),
    };
    audit_and_notify(&mut tx, actor, "create", &record).await?;
    tx.commit().await?;
    Ok(record)
}

/// Removes an entry; [`DbError::NotFound`] if it does not exist.
pub async fn delete_entry(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Uuid,
) -> Result<EntryRecord, DbError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as!(
        EntryRow,
        r#"SELECT e.id, p.key AS "project_key?", e.kind, e.host, e.port, e.namespace, e.service,
                  e.ports, e.allow_dns
           FROM egress_allowlist e LEFT JOIN projects p ON p.id = e.project_id
           WHERE e.id = $1 FOR UPDATE OF e"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("egress entry `{id}`")))?;
    let record = EntryRecord::try_from(row)?;
    sqlx::query!("DELETE FROM egress_allowlist WHERE id = $1", id)
        .execute(&mut *tx)
        .await?;
    audit_and_notify(&mut tx, actor, "delete", &record).await?;
    tx.commit().await?;
    Ok(record)
}

async fn audit_and_notify(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: Actor<'_>,
    verb: &str,
    record: &EntryRecord,
) -> Result<(), DbError> {
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        format!("egress_allowlist.{verb}"),
        "egress_allowlist",
        record.id.to_string(),
    )
    .with_details(record)?;
    write_audit_entry(&mut **tx, &entry).await?;
    publish(&mut **tx, &Notification::EgressPolicy).await
}

/// A configured cluster service egress rule from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterServiceRule {
    pub namespace: String,
    pub service: String,
    pub ports: Vec<u16>,
    pub allow_dns: bool,
}

/// Lists all `cluster-service` rules that apply to an instance and optional project key.
///
/// Instance-scoped rules always apply; project-scoped rules only for `project_key`. A stored port
/// outside `1..=65535` fails the whole call, so a corrupt row is not silently dropped from a
/// `NetworkPolicy`.
pub async fn list_cluster_services(
    pool: &PgPool,
    project_key: Option<&str>,
) -> Result<Vec<ClusterServiceRule>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT e.namespace AS "namespace!", e.service AS "service!", e.ports, e.allow_dns
           FROM egress_allowlist e
           LEFT JOIN projects p ON e.project_id = p.id
           WHERE e.kind = 'cluster-service'
             AND (e.scope = 'instance' OR (e.scope = 'project' AND p.key = $1))
           ORDER BY e.namespace, e.service"#,
        project_key
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ClusterServiceRule {
                namespace: row.namespace,
                service: row.service,
                ports: row
                    .ports
                    .into_iter()
                    .map(port_from_db)
                    .collect::<Result<_, _>>()?,
                allow_dns: row.allow_dns,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

fn port_from_db(port: i32) -> Result<u16, sqlx::Error> {
    u16::try_from(port).ok().filter(|p| *p != 0).ok_or_else(|| {
        sqlx::Error::Decode(format!("port {port} in egress allowlist is outside 1..=65535").into())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn port_from_db_accepts_only_valid_ports() {
        assert_eq!(port_from_db(1).unwrap(), 1);
        assert_eq!(port_from_db(65535).unwrap(), 65535);
        for invalid in [0, -1, 65536, i32::MAX] {
            assert!(port_from_db(invalid).is_err(), "{invalid} accepted");
        }
    }

    async fn insert_project(pool: &PgPool, key: &str) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image) \
             VALUES ($1, $2, $2, 'https://example.test/r.git', 'img:latest')",
        )
        .bind(id)
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn insert_rule(
        pool: &PgPool,
        scope: &str,
        project_id: Option<Uuid>,
        kind: &str,
        service: &str,
        ports: &[i32],
    ) {
        let is_http = kind == "http-host";
        sqlx::query(
            "INSERT INTO egress_allowlist \
                 (id, scope, project_id, kind, host, port, namespace, service, ports, allow_dns) \
             VALUES ($1, $2, $3, $4, $5, $6, 'ns', $7, $8, $9)",
        )
        .bind(Uuid::now_v7())
        .bind(scope)
        .bind(project_id)
        .bind(kind)
        .bind(is_http.then_some("example.test"))
        .bind(is_http.then_some(443_i32))
        .bind(service)
        .bind(ports)
        .bind(scope == "instance")
        .execute(pool)
        .await
        .unwrap();
    }

    fn services(rules: &[ClusterServiceRule]) -> Vec<&str> {
        rules.iter().map(|r| r.service.as_str()).collect()
    }

    // Needs a PostgreSQL container (testcontainers); not runnable in a sandbox without a
    // container socket.
    #[tokio::test]
    async fn rules_are_filtered_by_scope_and_project() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let alpha = insert_project(&pool, "ALPHA").await;
        let beta = insert_project(&pool, "BETA").await;
        insert_rule(
            &pool,
            "instance",
            None,
            "cluster-service",
            "shared",
            &[50051],
        )
        .await;
        insert_rule(
            &pool,
            "project",
            Some(alpha),
            "cluster-service",
            "alpha-only",
            &[80, 443],
        )
        .await;
        insert_rule(
            &pool,
            "project",
            Some(beta),
            "cluster-service",
            "beta-only",
            &[],
        )
        .await;
        insert_rule(
            &pool,
            "project",
            Some(alpha),
            "http-host",
            "not-a-cluster-service",
            &[],
        )
        .await;

        let for_alpha = list_cluster_services(&pool, Some("ALPHA")).await.unwrap();
        assert_eq!(services(&for_alpha), ["alpha-only", "shared"]);
        assert_eq!(for_alpha[0].ports, [80, 443]);
        assert!(!for_alpha[0].allow_dns);
        assert_eq!(for_alpha[1].ports, [50051]);
        assert!(for_alpha[1].allow_dns);

        let for_beta = list_cluster_services(&pool, Some("BETA")).await.unwrap();
        assert_eq!(services(&for_beta), ["beta-only", "shared"]);
        assert!(for_beta[0].ports.is_empty());

        let instance_only = list_cluster_services(&pool, None).await.unwrap();
        assert_eq!(services(&instance_only), ["shared"]);
    }

    #[tokio::test]
    async fn out_of_range_stored_port_fails_the_listing() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        insert_rule(&pool, "instance", None, "cluster-service", "bad", &[0]).await;

        assert!(list_cluster_services(&pool, None).await.is_err());
    }

    fn actor_fixture() -> (df_core::Principal, Vec<String>) {
        (
            df_core::Principal::human(df_core::UserId::new(), "Peter"),
            vec!["board-member".to_string()],
        )
    }

    fn http_host(host: &str, port: u16) -> EntryKind {
        EntryKind::HttpHost {
            host: host.into(),
            port,
        }
    }

    #[tokio::test]
    async fn entries_are_added_listed_deleted_audited_and_announced() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        insert_project(&pool, "DF").await;
        let notifier = crate::notify::Notifier::start(&pool).await.unwrap();
        let mut notifications = notifier.subscribe();
        let (principal, roles) = actor_fixture();
        let actor = Actor {
            principal: &principal,
            roles: &roles,
        };

        add_entry(&pool, actor, None, &http_host("api.anthropic.com", 443))
            .await
            .unwrap();
        let project = add_entry(&pool, actor, Some("DF"), &http_host("pypi.org", 443))
            .await
            .unwrap();
        let service = EntryKind::ClusterService {
            namespace: "nativelink".into(),
            service: "scheduler".into(),
            ports: vec![50051],
            allow_dns: false,
        };
        add_entry(&pool, actor, None, &service).await.unwrap();

        let announced =
            tokio::time::timeout(std::time::Duration::from_secs(5), notifications.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(announced, Notification::EgressPolicy);

        let listed = list_entries(&pool).await.unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].project_key, None);
        assert_eq!(listed[2], project);
        assert!(listed.iter().any(|e| e.kind == service));

        let duplicate = add_entry(&pool, actor, None, &http_host("api.anthropic.com", 443)).await;
        assert!(matches!(duplicate, Err(DbError::Conflict(_))));
        // The same host in another scope or on another port is a different entry.
        add_entry(&pool, actor, None, &http_host("api.anthropic.com", 80))
            .await
            .unwrap();
        let unknown_project = add_entry(&pool, actor, Some("NOPE"), &http_host("a.b", 443)).await;
        assert!(matches!(unknown_project, Err(DbError::NotFound(_))));

        // Drain what the adds announced, then expect the delete's notification.
        while notifications.try_recv().is_ok() {}
        delete_entry(&pool, actor, project.id).await.unwrap();
        let announced =
            tokio::time::timeout(std::time::Duration::from_secs(5), notifications.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(announced, Notification::EgressPolicy);
        assert!(matches!(
            delete_entry(&pool, actor, project.id).await,
            Err(DbError::NotFound(_))
        ));

        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM audit_log WHERE resource_type = 'egress_allowlist' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            actions
                .iter()
                .filter(|a| *a == "egress_allowlist.create")
                .count(),
            4
        );
        assert_eq!(
            actions.last().map(String::as_str),
            Some("egress_allowlist.delete")
        );
        let details: serde_json::Value = sqlx::query_scalar(
            "SELECT details FROM audit_log WHERE action = 'egress_allowlist.delete'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(details["host"], "pypi.org");
        assert_eq!(details["project_key"], "DF");
    }

    #[tokio::test]
    async fn snapshot_inputs_are_active_runs_valid_entries_and_rising_versions() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let project = insert_project(&pool, "DF").await;
        let persona = Uuid::now_v7();
        sqlx::query("INSERT INTO personas (id, name, system_prompt) VALUES ($1, 'p', 'x')")
            .bind(persona)
            .execute(&pool)
            .await
            .unwrap();
        for (number, state, pod_ip) in [
            (1_i64, "running", Some("10.1.0.7")),
            (2, "running", None),
            (3, "failed", Some("10.1.0.9")),
        ] {
            let task = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id)
                 VALUES ($1, $2, $3, 'DF-' || $3::text, 't', 'user', $4)",
            )
            .bind(task)
            .bind(project)
            .bind(number)
            .bind(persona)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, 1, $3, $4)",
            )
            .bind(Uuid::now_v7())
            .bind(task)
            .bind(state)
            .bind(pod_ip)
            .execute(&pool)
            .await
            .unwrap();
        }
        let runs = active_runs(&pool).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(
            (runs[0].pod_ip.as_str(), runs[0].task_key.as_str()),
            ("10.1.0.7", "DF-1")
        );
        assert_eq!(runs[0].run_number, 1);
        assert_eq!(runs[0].project_id, project);

        insert_rule(&pool, "instance", None, "http-host", "x", &[]).await;
        // A corrupt port narrows the policy: the entry is left out.
        sqlx::query(
            "INSERT INTO egress_allowlist (id, scope, kind, host, port) VALUES ($1, 'instance', 'http-host', 'bad.example', 70000)",
        )
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();
        let entries = http_host_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].host.as_str(), entries[0].port),
            ("example.test", 443)
        );

        let first = next_policy_version(&pool).await.unwrap();
        assert!(next_policy_version(&pool).await.unwrap() > first);
    }
}
