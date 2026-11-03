//! Deny rules (iter2-egress-deny-rules): `http-deny` entries of `egress_allowlist`. They silence
//! a host in the control plane (no request is raised or accepted for it) and change nothing in
//! the gateway's policy.

use df_core::RequestId;
use sqlx::{PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{EntryKind, EntryRecord, lock_allowlist, record_entry_change};
use crate::catalog::Actor;
use crate::pool::DbError;

/// The first deny rule pattern that matches `host`, on any port.
pub async fn deny_rule_for(
    executor: impl PgExecutor<'_>,
    host: &str,
) -> Result<Option<String>, DbError> {
    Ok(deny_patterns(executor)
        .await?
        .into_iter()
        .find(|pattern| df_core::host::pattern_matches(pattern, host)))
}

/// All deny rule patterns, oldest first.
pub async fn deny_patterns(executor: impl PgExecutor<'_>) -> Result<Vec<String>, DbError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT host AS "host!" FROM egress_allowlist
           WHERE kind = 'http-deny' AND host IS NOT NULL ORDER BY created_at, id"#
    )
    .fetch_all(executor)
    .await?)
}

/// [`create_deny_rule`] in a transaction of its own.
pub async fn add_deny_rule(
    pool: &PgPool,
    host: &str,
    actor: Actor<'_>,
) -> Result<EntryRecord, DbError> {
    let mut tx = pool.begin().await?;
    let record = create_deny_rule(&mut tx, host, actor, None).await?;
    tx.commit().await?;
    Ok(record)
}

/// Creates a deny rule for `host` (as returned by [`df_core::host::validate_allowlist_host`])
/// under the allowlist lock, audits it and denies the pending `network-access` requests it
/// matches as the system principal. `deciding` is a request the caller denied itself. The rule
/// is not published: the gateway does not know it. An existing rule for the host is a
/// [`DbError::Conflict`].
pub async fn create_deny_rule(
    tx: &mut Transaction<'_, Postgres>,
    host: &str,
    actor: Actor<'_>,
    deciding: Option<RequestId>,
) -> Result<EntryRecord, DbError> {
    lock_allowlist(tx).await?;
    let duplicate = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM egress_allowlist
             WHERE kind = 'http-deny' AND host = $1) AS "exists!""#,
        host
    )
    .fetch_one(&mut **tx)
    .await?;
    if duplicate {
        return Err(DbError::Conflict("this deny rule already exists".into()));
    }
    let id = Uuid::now_v7();
    sqlx::query!(
        "INSERT INTO egress_allowlist (id, scope, kind, host) VALUES ($1, 'instance', 'http-deny', $2)",
        id,
        host
    )
    .execute(&mut **tx)
    .await?;
    let record = EntryRecord {
        id,
        project_key: None,
        task_key: None,
        kind: EntryKind::HttpDeny {
            host: host.to_string(),
        },
    };
    record_entry_change(tx, actor, "create", &record).await?;
    crate::network_access::deny_matching(tx, host, deciding).await?;
    Ok(record)
}

#[cfg(test)]
mod tests;
