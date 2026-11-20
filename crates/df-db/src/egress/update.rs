//! Editing entries in place and replacing the project rules of a host by one instance rule
//! (egress-rule-editing design D2, D6).

use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use super::deny::ensure_no_deny_duplicate;
use super::{
    EntryKind, EntryRecord, HostScope, create_http_host_entry, delete_entry_in,
    ensure_no_cluster_service_duplicate, ensure_no_http_host_duplicate, lock_allowlist, lock_entry,
    record_entry_change, scope_keys,
};
use crate::catalog::Actor;
use crate::pool::DbError;

/// Changes the scope and the values of entry `id` in place, under the allowlist lock, with the
/// guarantees of creation: duplicate check, audit (`<prefix>.update` with the old and the new
/// values), `egress_policy` and settling of the pending `network-access` requests the new values
/// cover or match. `kind` must be of the entry's kind (the effect does not change); a task entry
/// stays with its task or moves to another scope; cluster services have no task scope and deny
/// rules are instance rules ([`DbError::InvalidData`]). A missing entry is
/// [`DbError::NotFound`], a duplicate [`DbError::Conflict`].
pub async fn update_entry(
    pool: &PgPool,
    actor: Actor<'_>,
    id: Uuid,
    scope: HostScope,
    kind: &EntryKind,
) -> Result<EntryRecord, DbError> {
    let mut tx = pool.begin().await?;
    lock_allowlist(&mut tx).await?;
    let row = lock_entry(&mut tx, id).await?;
    let before = EntryRecord::try_from(row)?;
    check_edit(&before.kind, kind, scope, before.task_id)?;

    let keys = scope_keys(&mut tx, scope).await?;
    match kind {
        EntryKind::HttpHost { host, port } => {
            ensure_no_http_host_duplicate(&mut tx, host, *port, scope, Some(id)).await?;
        }
        EntryKind::ClusterService {
            namespace, service, ..
        } => {
            ensure_no_cluster_service_duplicate(
                &mut tx,
                namespace,
                service,
                scope.project_id(),
                Some(id),
            )
            .await?;
        }
        EntryKind::HttpDeny { host } => ensure_no_deny_duplicate(&mut tx, host, Some(id)).await?,
    }

    // Columns a kind does not use stay NULL, empty or false, as on creation.
    let (host, port, namespace, service, ports, allow_dns) = match kind {
        EntryKind::HttpHost { host, port } => (
            Some(host),
            Some(i32::from(*port)),
            None,
            None,
            Vec::new(),
            false,
        ),
        EntryKind::ClusterService {
            namespace,
            service,
            ports,
            allow_dns,
        } => (
            None,
            None,
            Some(namespace),
            Some(service),
            ports.iter().map(|p| i32::from(*p)).collect(),
            *allow_dns,
        ),
        EntryKind::HttpDeny { host } => (Some(host), None, None, None, Vec::new(), false),
    };
    sqlx::query!(
        "UPDATE egress_allowlist
         SET scope = $2, project_id = $3, task_id = $4, host = $5, port = $6, namespace = $7,
             service = $8, ports = $9, allow_dns = $10
         WHERE id = $1",
        id,
        scope.as_str(),
        scope.project_id(),
        scope.task_id(),
        host,
        port,
        namespace,
        service,
        &ports,
        bool::from(allow_dns)
    )
    .execute(&mut *tx)
    .await?;

    let after = EntryRecord {
        id,
        project_key: keys.project,
        task_key: keys.task,
        task_id: scope.task_id(),
        task_project_key: keys.task_project,
        kind: kind.clone(),
    };
    let details = Change {
        before: &before,
        after: &after,
    };
    record_entry_change(&mut tx, actor, "update", &after, &details).await?;
    match &after.kind {
        EntryKind::HttpHost { host, port } => {
            crate::network_access::settle(&mut tx, scope, host, *port, actor, None).await?;
        }
        EntryKind::HttpDeny { host } => {
            crate::network_access::deny_matching(&mut tx, host, None).await?;
        }
        EntryKind::ClusterService { .. } => {}
    }
    tx.commit().await?;
    Ok(after)
}

/// Replaces all project rules (task rules stay) for `host` and `port` by one instance rule in one
/// transaction: every deletion and the creation are audited, one `egress_policy` is published.
/// No project rule is [`DbError::NotFound`]; an instance rule for `host` and `port` makes the
/// creation a [`DbError::Conflict`] and rolls everything back.
pub async fn make_global(
    pool: &PgPool,
    actor: Actor<'_>,
    host: &str,
    port: u16,
) -> Result<EntryRecord, DbError> {
    let mut tx = pool.begin().await?;
    lock_allowlist(&mut tx).await?;
    let ids = sqlx::query_scalar!(
        "SELECT id FROM egress_allowlist
         WHERE kind = 'http-host' AND host = $1 AND port = $2
           AND project_id IS NOT NULL AND task_id IS NULL
         ORDER BY created_at, id",
        host,
        i32::from(port)
    )
    .fetch_all(&mut *tx)
    .await?;
    if ids.is_empty() {
        return Err(DbError::NotFound(format!(
            "project rules for {host}:{port}"
        )));
    }
    for id in ids {
        delete_entry_in(&mut tx, actor, id).await?;
    }
    let record =
        create_http_host_entry(&mut tx, HostScope::Instance, host, port, actor, None).await?;
    tx.commit().await?;
    Ok(record)
}

/// The audit details of an edit.
#[derive(Serialize)]
struct Change<'a> {
    before: &'a EntryRecord,
    after: &'a EntryRecord,
}

/// Rejects an edit the table constraint or the rules of the feature forbid.
fn check_edit(
    old: &EntryKind,
    new: &EntryKind,
    scope: HostScope,
    own_task: Option<Uuid>,
) -> Result<(), DbError> {
    let invalid = |reason: &str| Err(DbError::InvalidData(reason.to_string()));
    if std::mem::discriminant(old) != std::mem::discriminant(new) {
        return invalid("the kind of a rule cannot be changed");
    }
    match (new, scope) {
        (EntryKind::ClusterService { .. }, HostScope::Task(_)) => {
            invalid("a cluster service has no task scope")
        }
        (EntryKind::HttpDeny { .. }, HostScope::Project(_) | HostScope::Task(_)) => {
            invalid("a deny rule applies to the instance")
        }
        (_, HostScope::Task(task)) if own_task != Some(task) => {
            invalid("a rule cannot move to another task")
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
