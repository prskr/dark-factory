//! The `network-access` approval type (iter2-egress-approvals, `specs/network-access-requests`):
//! an agent or the control plane asks for `host:port`, and an approval creates the `http-host`
//! entry. New entries settle the pending requests they cover (design D3).

use df_core::host::{
    DEFAULT_HOST_PORT, pattern_matches, validate_allowlist_host, validate_exact_host,
};
use df_core::{Principal, RequestId, RunId, RunState, TaskId, TaskStatus};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};

use crate::catalog::Actor;
use crate::egress::{HostScope, allowlist_covers, create_http_host_entry, lock_allowlist};
use crate::pool::DbError;
use crate::requests::{
    ApprovalType, NewRequest, Raise, Raised, RequestError, RequestKind, RequestRow, Scope,
    raise_in, record_approval_in,
};
use crate::tasks::TaskRow;

/// Name of the approval type.
pub const NETWORK_ACCESS: &str = "network-access";

/// The `network-access` approval type.
pub struct NetworkAccess;

/// The normalized `(host, port)` of a `network-access` payload `{host, port}`; the port defaults
/// to 443.
pub fn requested_target(payload: &Value) -> Result<(String, u16), RequestError> {
    let invalid = |what: &str| RequestError::Invalid(what.to_string());
    let host = payload
        .get("host")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("`host` is required"))?;
    let host = validate_allowlist_host(host).map_err(invalid)?;
    let port = match payload.get("port") {
        None | Some(Value::Null) => DEFAULT_HOST_PORT,
        Some(port) => port
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p != 0)
            .ok_or_else(|| invalid("`port` must be a number between 1 and 65535"))?,
    };
    Ok((host, port))
}

/// The stored payload of a `network-access` request.
fn target_payload(host: &str, port: u16) -> Value {
    json!({ "host": host, "port": port })
}

#[async_trait::async_trait]
impl ApprovalType for NetworkAccess {
    fn name(&self) -> &'static str {
        NETWORK_ACCESS
    }

    fn validate(&self, _task: &TaskRow, payload: &Value) -> Result<Value, RequestError> {
        requested_target(payload).map(|(host, port)| target_payload(&host, port))
    }

    fn fingerprint(&self, payload: &Value) -> String {
        requested_target(payload)
            .map_or_else(|_invalid| payload.to_string(), |(h, p)| format!("{h}:{p}"))
    }

    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Task, Scope::Project, Scope::Instance]
    }

    async fn lock(&self, tx: &mut Transaction<'_, Postgres>) -> Result<(), DbError> {
        lock_allowlist(tx).await
    }

    async fn granted(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        task: &TaskRow,
        payload: &Value,
    ) -> Result<bool, RequestError> {
        let (host, port) = requested_target(payload)?;
        Ok(allowlist_covers(tx, task.id.as_uuid(), &host, port).await?)
    }

    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        scope: Scope,
        decider: Actor<'_>,
    ) -> Result<(), RequestError> {
        let (host, port) = requested_target(&request.payload)?;
        let task_id = request.task_id.as_uuid();
        let project_id = sqlx::query_scalar!("SELECT project_id FROM tasks WHERE id = $1", task_id)
            .fetch_one(&mut **tx)
            .await?;
        let scope = HostScope::for_approval(scope, task_id, project_id)
            .ok_or(RequestError::ScopeNotOffered(scope))?;
        match create_http_host_entry(tx, scope, &host, port, decider, Some(request.id)).await {
            // The entry exists already: what the approval asks for is in place.
            Ok(_) | Err(DbError::Conflict(_)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn describe(&self, request: &RequestRow) -> String {
        match requested_target(&request.payload) {
            Ok((host, port)) => format!("network access to {host}:{port}"),
            Err(_invalid) => "network access".to_string(),
        }
    }
}

/// What [`raise_from_denial`] did with a denial report.
#[derive(Debug, Clone, PartialEq)]
pub enum DenialOutcome {
    /// The request, or [`Raised::AlreadyGranted`] if the run's effective allowlist covers host
    /// and port now.
    Raised(Raised),
    /// No run has this id.
    UnknownRun,
    /// The run is no longer active.
    RunEnded,
    /// The run's task is completed.
    TaskCompleted,
}

/// Raises a non-blocking `network-access` request, raised by the control plane, for a
/// `not-allowlisted` denial of `run_id` reported by the gateway (design D2). Takes the allowlist
/// lock and then the task row lock, like entry creation and marking the task done, so the request
/// is either settled or closed by them or not raised at all.
pub async fn raise_from_denial(
    pool: &PgPool,
    run_id: RunId,
    host: &str,
    port: u32,
) -> Result<DenialOutcome, RequestError> {
    let (host, port) = requested_target(&json!({ "host": host, "port": port }))?;
    validate_exact_host(&host).map_err(|error| RequestError::Invalid(error.into()))?;
    let Some(task_id) =
        sqlx::query_scalar!("SELECT task_id FROM runs WHERE id = $1", run_id.as_uuid())
            .fetch_optional(pool)
            .await?
    else {
        return Ok(DenialOutcome::UnknownRun);
    };

    let mut tx = pool.begin().await?;
    lock_allowlist(&mut tx).await?;
    let status = sqlx::query_scalar!("SELECT status FROM tasks WHERE id = $1 FOR UPDATE", task_id)
        .fetch_one(&mut *tx)
        .await?;
    let state = sqlx::query_scalar!("SELECT state FROM runs WHERE id = $1", run_id.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
    if state
        .parse::<RunState>()
        .map_err(DbError::from)?
        .is_terminal()
    {
        return Ok(DenialOutcome::RunEnded);
    }
    if status.parse::<TaskStatus>().map_err(DbError::from)? == TaskStatus::Completed {
        return Ok(DenialOutcome::TaskCompleted);
    }
    if allowlist_covers(&mut tx, task_id, &host, port).await? {
        return Ok(DenialOutcome::Raised(Raised::AlreadyGranted));
    }
    let actor = Principal::system("egress.denied");
    let reason = format!("The egress proxy denied {host}:{port}");
    let payload = target_payload(&host, port);
    let fingerprint = NetworkAccess.fingerprint(&payload);
    let (request, new) = raise_in(
        &mut tx,
        NewRequest {
            from: Raise {
                task_id: TaskId::from_uuid(task_id),
                run_id: None,
                actor: &actor,
                blocking: false,
            },
            kind: RequestKind::Approval,
            approval_type: Some(NETWORK_ACCESS.to_string()),
            payload,
            reason: &reason,
            fingerprint,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(DenialOutcome::Raised(Raised::Request {
        request: Box::new(request),
        new,
    }))
}

/// Approves, with `actor` as decider and without applying them again, the pending
/// `network-access` requests that the new entry (`scope`, `host`, `port`) covers, except
/// `deciding`. Runs in the entry's transaction, under the allowlist lock.
pub(crate) async fn settle(
    tx: &mut Transaction<'_, Postgres>,
    scope: HostScope,
    host: &str,
    port: u16,
    actor: Actor<'_>,
    deciding: Option<RequestId>,
) -> Result<(), DbError> {
    let pending = sqlx::query!(
        "SELECT r.id, r.payload FROM agent_requests r JOIN tasks t ON t.id = r.task_id
         WHERE r.state = 'pending' AND r.kind = 'approval' AND r.approval_type = $1
           AND ($2::uuid IS NULL OR r.task_id = $2) AND ($3::uuid IS NULL OR t.project_id = $3)
         ORDER BY r.created_at, r.id",
        NETWORK_ACCESS,
        scope.task_id(),
        scope.project_id()
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in pending {
        let id = RequestId::from_uuid(row.id);
        let covered = requested_target(&row.payload)
            .is_ok_and(|(h, p)| p == port && pattern_matches(host, &h));
        if covered && deciding != Some(id) {
            record_approval_in(tx, &NetworkAccess, id, actor, scope.into()).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
