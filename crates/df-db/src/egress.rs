//! Egress allowlist persistence operations.

use sqlx::PgPool;

use crate::pool::DbError;

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
/// NetworkPolicy.
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
}
