//! Agent requests: questions and typed approvals (`specs/agent-requests/spec.md`, ADR-0038).
//!
//! Raising merges identical pending requests and enforces the pending limit. Deciding applies the
//! approval type's action, stores the decision, audits it and queues the `request.decided`
//! message in one transaction. Later changes add approval types by implementing [`ApprovalType`]
//! and registering it in [`ApprovalTypes`].

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use df_core::{Principal, RequestId, RunId, TaskId};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::notify::{Notification, publish};
use crate::pool::DbError;
use crate::tasks::{TaskRow, get_task, queue_message_tx};

/// Pending requests one task may have at a time.
pub const MAX_PENDING_PER_TASK: i64 = 20;
/// Longest question text and answer, in characters.
pub const MAX_TEXT_CHARS: usize = 4000;
/// Most suggested answers of a question.
pub const MAX_SUGGESTED_ANSWERS: usize = 5;
/// Longest suggested answer, in characters.
pub const MAX_SUGGESTED_ANSWER_CHARS: usize = 200;

macro_rules! columns {
    () => {
        "id, task_id, run_id, kind, approval_type, payload, reason, blocking, \
     state, decision, decided_by, decided_at, close_reason, created_at"
    };
}

/// Why a request operation failed. The messages are shown to agents and humans.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("unknown approval type `{0}`")]
    UnknownType(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error(
        "too many pending requests (at most {MAX_PENDING_PER_TASK} per task); wait for decisions"
    )]
    TooManyPending,
    #[error("request not found")]
    NotFound,
    #[error("already decided")]
    AlreadyDecided,
    #[error("the approval type does not offer scope `{0}`")]
    ScopeNotOffered(Scope),
    /// The approval type's action failed; the decision was not stored.
    #[error("the approved action failed: {0}")]
    ApplyFailed(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<sqlx::Error> for RequestError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e.into())
    }
}

impl From<serde_json::Error> for RequestError {
    fn from(e: serde_json::Error) -> Self {
        Self::Db(e.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Question,
    Approval,
}

impl RequestKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Approval => "approval",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Pending,
    Answered,
    Approved,
    Denied,
    Closed,
}

impl RequestState {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Closed => "closed",
        }
    }
}

impl fmt::Display for RequestState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How far an approval reaches, narrowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Run,
    Task,
    Project,
    Instance,
}

impl Scope {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Task => "task",
            Self::Project => "project",
            Self::Instance => "instance",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scope {
    type Err = RequestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "run" => Ok(Self::Run),
            "task" => Ok(Self::Task),
            "project" => Ok(Self::Project),
            "instance" => Ok(Self::Instance),
            other => Err(RequestError::Invalid(format!("unknown scope `{other}`"))),
        }
    }
}

/// A stored request.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestRow {
    pub id: RequestId,
    pub task_id: TaskId,
    /// `None` when the control plane raised the request.
    pub run_id: Option<RunId>,
    pub kind: RequestKind,
    pub approval_type: Option<String>,
    pub payload: Value,
    pub reason: String,
    pub blocking: bool,
    pub state: RequestState,
    pub decision: Option<Value>,
    pub decided_by: Option<Principal>,
    pub decided_at: Option<DateTime<Utc>>,
    pub close_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl RequestRow {
    /// The text of a question.
    pub fn question_text(&self) -> &str {
        self.payload
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// The suggested answers of a question.
    pub fn suggested_answers(&self) -> Vec<&str> {
        self.payload
            .get("suggested_answers")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The answer of an answered question.
    pub fn answer(&self) -> Option<&str> {
        self.decision.as_ref()?.get("answer")?.as_str()
    }

    /// The scope of an approved request.
    pub fn scope(&self) -> Option<Scope> {
        self.decision.as_ref()?.get("scope")?.as_str()?.parse().ok()
    }

    /// The comment of a denial.
    pub fn comment(&self) -> Option<&str> {
        self.decision.as_ref()?.get("comment")?.as_str()
    }
}

/// One kind of approval request. Implementations are registered at start-up.
#[async_trait::async_trait] // boxes the future so the trait stays dyn-compatible
pub trait ApprovalType: Send + Sync {
    fn name(&self) -> &'static str;
    /// Rejects a payload that does not fit the type or the task.
    fn validate(&self, task: &TaskRow, payload: &Value) -> Result<(), RequestError>;
    /// Identical pending requests of a task (same fingerprint) are merged.
    fn fingerprint(&self, payload: &Value) -> String;
    /// The scopes a human may choose from, narrowest first.
    fn scopes(&self) -> &'static [Scope];
    /// The action an approval takes, in the decision's transaction.
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        scope: Scope,
    ) -> Result<(), RequestError>;
    /// Text for the UI card and the decision message.
    fn describe(&self, request: &RequestRow) -> String;
}

/// The approval types of this instance.
#[derive(Clone, Default)]
pub struct ApprovalTypes(BTreeMap<&'static str, Arc<dyn ApprovalType>>);

impl ApprovalTypes {
    #[must_use]
    pub fn with(mut self, approval_type: Arc<dyn ApprovalType>) -> Self {
        self.0.insert(approval_type.name(), approval_type);
        self
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ApprovalType>> {
        self.0.get(name)
    }
}

/// A human's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Answer(String),
    Approve(Scope),
    Deny(Option<String>),
}

/// Where a request comes from and how it behaves.
#[derive(Debug, Clone, Copy)]
pub struct Raise<'a> {
    pub task_id: TaskId,
    /// `None` when the control plane raises the request.
    pub run_id: Option<RunId>,
    pub actor: &'a Principal,
    pub blocking: bool,
}

/// Raises a question on the task. Returns the request and whether it is new; an identical
/// pending question of the task is returned instead of a new one.
pub async fn raise_question(
    pool: &PgPool,
    from: Raise<'_>,
    text: &str,
    suggested_answers: &[String],
) -> Result<(RequestRow, bool), RequestError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
        return Err(RequestError::Invalid(format!(
            "the question must have 1 to {MAX_TEXT_CHARS} characters"
        )));
    }
    if suggested_answers.len() > MAX_SUGGESTED_ANSWERS {
        return Err(RequestError::Invalid(format!(
            "at most {MAX_SUGGESTED_ANSWERS} suggested answers"
        )));
    }
    let suggested_answers: Vec<String> = suggested_answers
        .iter()
        .map(|answer| answer.trim().to_string())
        .collect();
    for answer in &suggested_answers {
        let chars = answer.chars().count();
        if chars == 0 || chars > MAX_SUGGESTED_ANSWER_CHARS {
            return Err(RequestError::Invalid(format!(
                "a suggested answer must have 1 to {MAX_SUGGESTED_ANSWER_CHARS} characters"
            )));
        }
    }
    let payload = json!({ "text": text, "suggested_answers": suggested_answers });
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let fingerprint = hex::encode(Sha256::digest(normalized.as_bytes()));
    raise(
        pool,
        NewRequest {
            from,
            kind: RequestKind::Question,
            approval_type: None,
            payload,
            reason: "",
            fingerprint,
        },
    )
    .await
}

/// Raises an approval request of a registered type on the task.
pub async fn raise_approval(
    pool: &PgPool,
    types: &ApprovalTypes,
    from: Raise<'_>,
    type_name: &str,
    payload: Value,
    reason: &str,
) -> Result<(RequestRow, bool), RequestError> {
    let approval_type = types
        .get(type_name)
        .ok_or_else(|| RequestError::UnknownType(type_name.to_string()))?;
    let task = get_task(pool, from.task_id)
        .await?
        .ok_or(RequestError::NotFound)?;
    approval_type.validate(&task, &payload)?;
    let fingerprint = approval_type.fingerprint(&payload);
    raise(
        pool,
        NewRequest {
            from,
            kind: RequestKind::Approval,
            approval_type: Some(type_name.to_string()),
            payload,
            reason,
            fingerprint,
        },
    )
    .await
}

/// Decides a pending request (design D3). `decider` is the human, `roles` their roles for the
/// audit entry; the caller checked `approval:decide`.
pub async fn decide(
    pool: &PgPool,
    types: &ApprovalTypes,
    request_id: RequestId,
    decider: &Principal,
    roles: &[String],
    decision: Decision,
) -> Result<RequestRow, RequestError> {
    let mut tx = pool.begin().await?;
    let (task_id, request) = lock_pending_request(&mut tx, request_id).await?;
    let (state, stored, action) = apply_decision(&mut tx, types, &request, &decision).await?;

    let record = sqlx::query_as::<_, RequestRecord>(concat!(
        "UPDATE agent_requests
         SET state = $2, decision = $3, decided_by = $4, decided_at = now()
         WHERE id = $1 RETURNING ",
        columns!(),
        ""
    ))
    .bind(request_id.as_uuid())
    .bind(state.as_str())
    .bind(&stored)
    .bind(serde_json::to_value(decider)?)
    .fetch_one(&mut *tx)
    .await?;
    let decided: RequestRow = record.try_into()?;

    let entry = NewAuditEntry::new(
        decider.clone(),
        roles.to_vec(),
        action,
        "request",
        request_id.to_string(),
    )
    .with_details(&json!({ "task_id": task_id, "decision": stored }))?;
    write_audit_entry(&mut *tx, &entry).await?;

    let message = decision_message(types, &decided);
    queue_message_tx(
        &mut tx,
        task_id,
        Principal::system("request.decided"),
        vec![text_block(message)],
    )
    .await?;
    publish(
        &mut *tx,
        &Notification::RequestChanged {
            task_id,
            request_id,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(decided)
}

/// Locks the task and then the request (like `raise` and `close_pending_for_task`) and returns
/// them if the request is still pending.
async fn lock_pending_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: RequestId,
) -> Result<(TaskId, RequestRow), RequestError> {
    let task_uuid: uuid::Uuid =
        sqlx::query_scalar("SELECT task_id FROM agent_requests WHERE id = $1")
            .bind(request_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(RequestError::NotFound)?;
    let task_id = TaskId::from_uuid(task_uuid);
    lock_task(tx, task_id).await?;
    let record = sqlx::query_as::<_, RequestRecord>(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_requests WHERE id = $1 FOR UPDATE"
    ))
    .bind(request_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let request: RequestRow = record.try_into()?;
    if request.state != RequestState::Pending {
        return Err(RequestError::AlreadyDecided);
    }
    Ok((task_id, request))
}

/// Validates `decision` against the kind of `request` and applies an approval's effect. Returns
/// the new state, the stored decision and the audit action.
async fn apply_decision(
    tx: &mut Transaction<'_, Postgres>,
    types: &ApprovalTypes,
    request: &RequestRow,
    decision: &Decision,
) -> Result<(RequestState, Value, &'static str), RequestError> {
    Ok(match (decision, request.kind) {
        (Decision::Answer(text), RequestKind::Question) => {
            let text = text.trim();
            if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
                return Err(RequestError::Invalid(format!(
                    "the answer must have 1 to {MAX_TEXT_CHARS} characters"
                )));
            }
            (
                RequestState::Answered,
                json!({ "answer": text }),
                "request.answer",
            )
        }
        (Decision::Approve(scope), RequestKind::Approval) => {
            let approval_type = approval_type_of(types, request)?;
            if !approval_type.scopes().contains(scope) {
                return Err(RequestError::ScopeNotOffered(*scope));
            }
            approval_type.apply(tx, request, *scope).await?;
            (
                RequestState::Approved,
                json!({ "scope": scope.as_str() }),
                "request.approve",
            )
        }
        (Decision::Deny(comment), RequestKind::Approval) => {
            let comment = comment.as_deref().map(str::trim).filter(|c| !c.is_empty());
            (
                RequestState::Denied,
                json!({ "comment": comment }),
                "request.deny",
            )
        }
        _ => {
            return Err(RequestError::Invalid(
                "the decision does not fit the kind of the request".into(),
            ));
        }
    })
}

/// Closes the pending requests of a task (for example when it completes). Runs in the caller's
/// transaction; the notifications go out on commit. Returns the closed ids.
pub async fn close_pending_for_task(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    reason: &str,
    actor: &Principal,
) -> Result<Vec<RequestId>, DbError> {
    lock_task(tx, task_id).await?;
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE agent_requests SET state = 'closed', close_reason = $2
         WHERE task_id = $1 AND state = 'pending' RETURNING id",
    )
    .bind(task_id.as_uuid())
    .bind(reason)
    .fetch_all(&mut **tx)
    .await?;
    let mut closed = Vec::with_capacity(ids.len());
    for id in ids {
        let request_id = RequestId::from_uuid(id);
        let entry = NewAuditEntry::new(
            actor.clone(),
            Vec::new(),
            "request.close",
            "request",
            request_id.to_string(),
        )
        .with_details(&json!({ "task_id": task_id, "reason": reason }))?;
        write_audit_entry(&mut **tx, &entry).await?;
        publish(
            &mut **tx,
            &Notification::RequestChanged {
                task_id,
                request_id,
            },
        )
        .await?;
        closed.push(request_id);
    }
    Ok(closed)
}

/// The request with `id`.
pub async fn get_request(pool: &PgPool, id: RequestId) -> Result<Option<RequestRow>, DbError> {
    sqlx::query_as::<_, RequestRecord>(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_requests WHERE id = $1"
    ))
    .bind(id.as_uuid())
    .fetch_optional(pool)
    .await?
    .map(RequestRow::try_from)
    .transpose()
}

/// All pending requests with their task key and title, newest first.
pub async fn list_pending(pool: &PgPool) -> Result<Vec<(RequestRow, String, String)>, DbError> {
    let rows = sqlx::query_as::<_, RequestRecord>(
        "SELECT r.*, t.key AS task_key, t.title AS task_title
         FROM agent_requests r JOIN tasks t ON t.id = r.task_id
         WHERE r.state = 'pending' ORDER BY r.created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            let (key, title) = (
                r.task_key.clone().unwrap_or_default(),
                r.task_title.clone().unwrap_or_default(),
            );
            Ok((RequestRow::try_from(r)?, key, title))
        })
        .collect()
}

/// The requests of a task, oldest first.
pub async fn list_for_task(pool: &PgPool, task_id: TaskId) -> Result<Vec<RequestRow>, DbError> {
    sqlx::query_as::<_, RequestRecord>(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_requests WHERE task_id = $1 ORDER BY created_at, id"
    ))
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(RequestRow::try_from)
    .collect()
}

/// Number of pending requests across tasks.
pub async fn pending_count(pool: &PgPool) -> Result<i64, DbError> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM agent_requests WHERE state = 'pending'")
            .fetch_one(pool)
            .await?,
    )
}

/// The text of a decision message to the agent.
pub fn decision_message(types: &ApprovalTypes, request: &RequestRow) -> String {
    let subject = match request.kind {
        RequestKind::Question => format!("your question \"{}\"", request.question_text()),
        RequestKind::Approval => request
            .approval_type
            .as_deref()
            .and_then(|name| types.get(name))
            .map_or_else(
                || "your approval request".to_string(),
                |t| t.describe(request),
            ),
    };
    let who = match &request.decided_by {
        Some(Principal::Human { display_name, .. }) => format!(" by {display_name}"),
        _ => String::new(),
    };
    match request.state {
        RequestState::Answered => format!(
            "Request {} ({subject}) was answered{who}: {}",
            request.id,
            request.answer().unwrap_or_default()
        ),
        RequestState::Approved => format!(
            "Request {} ({subject}) was approved{who} with scope {}.",
            request.id,
            request.scope().map_or("-", |s| s.as_str())
        ),
        _ => match request.comment() {
            Some(c) => format!("Request {} ({subject}) was denied{who}: {c}", request.id),
            None => format!("Request {} ({subject}) was denied{who}.", request.id),
        },
    }
}

struct NewRequest<'a> {
    from: Raise<'a>,
    kind: RequestKind,
    approval_type: Option<String>,
    payload: Value,
    reason: &'a str,
    fingerprint: String,
}

async fn raise(pool: &PgPool, new: NewRequest<'_>) -> Result<(RequestRow, bool), RequestError> {
    let mut tx = pool.begin().await?;
    lock_task(&mut tx, new.from.task_id).await?;

    let existing = sqlx::query_as::<_, RequestRecord>(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_requests
         WHERE task_id = $1 AND kind = $2 AND COALESCE(approval_type, '') = COALESCE($3, '')
           AND fingerprint = $4 AND state = 'pending'"
    ))
    .bind(new.from.task_id.as_uuid())
    .bind(new.kind.as_str())
    .bind(&new.approval_type)
    .bind(&new.fingerprint)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(existing) = existing {
        return Ok((existing.try_into()?, false));
    }

    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_requests WHERE task_id = $1 AND state = 'pending'",
    )
    .bind(new.from.task_id.as_uuid())
    .fetch_one(&mut *tx)
    .await?;
    if pending >= MAX_PENDING_PER_TASK {
        return Err(RequestError::TooManyPending);
    }

    let id = RequestId::new();
    let record = sqlx::query_as::<_, RequestRecord>(concat!(
        "INSERT INTO agent_requests
             (id, task_id, run_id, kind, approval_type, payload, reason, blocking, fingerprint)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING ",
        columns!(),
        ""
    ))
    .bind(id.as_uuid())
    .bind(new.from.task_id.as_uuid())
    .bind(new.from.run_id.map(|r| r.as_uuid()))
    .bind(new.kind.as_str())
    .bind(&new.approval_type)
    .bind(&new.payload)
    .bind(new.reason)
    .bind(new.from.blocking)
    .bind(&new.fingerprint)
    .fetch_one(&mut *tx)
    .await?;
    let row: RequestRow = record.try_into()?;
    let entry = NewAuditEntry::new(
        new.from.actor.clone(),
        Vec::new(),
        "request.raise",
        "request",
        id.to_string(),
    )
    .with_details(&json!({
        "task_id": new.from.task_id,
        "kind": new.kind.as_str(),
        "approval_type": new.approval_type,
        "blocking": new.from.blocking,
    }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    publish(
        &mut *tx,
        &Notification::RequestChanged {
            task_id: new.from.task_id,
            request_id: id,
        },
    )
    .await?;
    tx.commit().await?;
    Ok((row, true))
}

fn approval_type_of<'a>(
    types: &'a ApprovalTypes,
    request: &RequestRow,
) -> Result<&'a Arc<dyn ApprovalType>, RequestError> {
    let name = request.approval_type.as_deref().unwrap_or_default();
    types
        .get(name)
        .ok_or_else(|| RequestError::UnknownType(name.to_string()))
}

fn text_block(text: String) -> ContentBlock {
    ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text })),
        hidden: false,
    }
}

async fn lock_task(tx: &mut Transaction<'_, Postgres>, task_id: TaskId) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct RequestRecord {
    id: uuid::Uuid,
    task_id: uuid::Uuid,
    run_id: Option<uuid::Uuid>,
    kind: String,
    approval_type: Option<String>,
    payload: Value,
    reason: String,
    blocking: bool,
    state: String,
    decision: Option<Value>,
    decided_by: Option<Value>,
    decided_at: Option<DateTime<Utc>>,
    close_reason: Option<String>,
    created_at: DateTime<Utc>,
    #[sqlx(default)]
    task_key: Option<String>,
    #[sqlx(default)]
    task_title: Option<String>,
}

impl TryFrom<RequestRecord> for RequestRow {
    type Error = DbError;

    fn try_from(r: RequestRecord) -> Result<Self, DbError> {
        let kind = match r.kind.as_str() {
            "question" => RequestKind::Question,
            "approval" => RequestKind::Approval,
            other => return Err(DbError::InvalidData(format!("request kind `{other}`"))),
        };
        let state = match r.state.as_str() {
            "pending" => RequestState::Pending,
            "answered" => RequestState::Answered,
            "approved" => RequestState::Approved,
            "denied" => RequestState::Denied,
            "closed" => RequestState::Closed,
            other => return Err(DbError::InvalidData(format!("request state `{other}`"))),
        };
        Ok(Self {
            id: RequestId::from_uuid(r.id),
            task_id: TaskId::from_uuid(r.task_id),
            run_id: r.run_id.map(RunId::from_uuid),
            kind,
            approval_type: r.approval_type,
            payload: r.payload,
            reason: r.reason,
            blocking: r.blocking,
            state,
            decision: r.decision,
            decided_by: r.decided_by.map(serde_json::from_value).transpose()?,
            decided_at: r.decided_at,
            close_reason: r.close_reason,
            created_at: r.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{NewTask, create_task};
    use df_core::{PersonaId, ProjectId, UserId};

    fn raise_by(task_id: TaskId, actor: &Principal, blocking: bool) -> Raise<'_> {
        Raise {
            task_id,
            run_id: None,
            actor,
            blocking,
        }
    }

    struct FakeType;

    #[async_trait::async_trait]
    impl ApprovalType for FakeType {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn validate(&self, _task: &TaskRow, payload: &Value) -> Result<(), RequestError> {
            payload
                .get("what")
                .and_then(Value::as_str)
                .map(|_| ())
                .ok_or_else(|| RequestError::Invalid("`what` is required".into()))
        }
        fn fingerprint(&self, payload: &Value) -> String {
            payload["what"].as_str().unwrap_or_default().to_string()
        }
        fn scopes(&self) -> &'static [Scope] {
            &[Scope::Task, Scope::Project]
        }
        async fn apply(
            &self,
            tx: &mut Transaction<'_, Postgres>,
            request: &RequestRow,
            _scope: Scope,
        ) -> Result<(), RequestError> {
            if request.payload["what"] == "fail" {
                return Err(RequestError::ApplyFailed("nope".into()));
            }
            sqlx::query("INSERT INTO audit_log (actor, roles, action, resource_type, resource_id, details) VALUES ('{}', '{}', 'fake.applied', 'request', $1, '{}')")
                .bind(request.id.to_string())
                .execute(&mut **tx)
                .await?;
            Ok(())
        }
        fn describe(&self, request: &RequestRow) -> String {
            format!("fake {}", request.payload["what"])
        }
    }

    fn types() -> ApprovalTypes {
        ApprovalTypes::default().with(Arc::new(FakeType))
    }

    async fn task(pool: &PgPool) -> TaskRow {
        let (project_id, persona_id) = (ProjectId::new(), PersonaId::new());
        sqlx::query("INSERT INTO projects (id, key, name, repo_url, image, base_branch) VALUES ($1, 'DF', 'P', 'https://x.test/r.git', 'img', 'main')")
            .bind(project_id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO personas (id, name, system_prompt, harness) VALUES ($1, 'Linus', 's', 'claude-code')")
            .bind(persona_id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
        create_task(
            pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("T".into()),
                description: "d".into(),
                issue_number: None,
                created_by: None,
            },
        )
        .await
        .unwrap()
        .0
    }

    fn agent(task: &TaskRow) -> Principal {
        Principal::agent(PersonaId::new(), "Linus", None, task.id)
    }

    fn human() -> Principal {
        Principal::human(UserId::new(), "Peter")
    }

    async fn audit_count(pool: &PgPool, action: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
            .bind(action)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn blocked_is_an_accepted_task_status() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        sqlx::query("UPDATE tasks SET status = 'blocked' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            sqlx::query("UPDATE tasks SET status = 'bogus' WHERE id = $1")
                .bind(task.id.as_uuid())
                .execute(&pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn question_is_stored_and_identical_ones_merge() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let answers = vec!["16".to_string(), "17".to_string()];
        let (first, created) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Use Postgres 16 or 17?",
            &answers,
        )
        .await
        .unwrap();
        assert!(created);
        assert_eq!(first.state, RequestState::Pending);
        assert_eq!(first.suggested_answers(), ["16", "17"]);
        let (second, created) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), true),
            "  use postgres 16   or 17? ",
            &[],
        )
        .await
        .unwrap();
        assert!(!created);
        assert_eq!(second.id, first.id);
        assert_eq!(pending_count(&pool).await.unwrap(), 1);
        assert_eq!(audit_count(&pool, "request.raise").await, 1);
    }

    #[tokio::test]
    async fn unknown_type_and_invalid_payload_store_nothing() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let err = raise_approval(
            &pool,
            &types(),
            raise_by(task.id, &agent(&task), false),
            "delete-everything",
            json!({}),
            "r",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RequestError::UnknownType(_)));
        let err = raise_approval(
            &pool,
            &types(),
            raise_by(task.id, &agent(&task), false),
            "fake",
            json!({}),
            "r",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RequestError::Invalid(_)));
        assert_eq!(pending_count(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn request_flood_is_limited_but_merging_still_works() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let actor = agent(&task);
        for i in 0..MAX_PENDING_PER_TASK {
            raise_question(
                &pool,
                raise_by(task.id, &actor, false),
                &format!("q{i}"),
                &[],
            )
            .await
            .unwrap();
        }
        let err = raise_question(&pool, raise_by(task.id, &actor, false), "one more", &[])
            .await
            .unwrap_err();
        assert!(matches!(err, RequestError::TooManyPending));
        let (_, created) = raise_question(&pool, raise_by(task.id, &actor, false), "q0", &[])
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(pending_count(&pool).await.unwrap(), MAX_PENDING_PER_TASK);
    }

    #[tokio::test]
    async fn answer_is_stored_audited_and_queued_as_a_message() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        sqlx::query("UPDATE tasks SET status = 'blocked' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let (q, _) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), true),
            "Which?",
            &["17".into()],
        )
        .await
        .unwrap();
        let peter = human();
        let decided = decide(
            &pool,
            &types(),
            q.id,
            &peter,
            &["board-member".into()],
            Decision::Answer("17".into()),
        )
        .await
        .unwrap();
        assert_eq!(decided.state, RequestState::Answered);
        assert_eq!(decided.answer(), Some("17"));
        assert_eq!(decided.decided_by, Some(peter));
        assert_eq!(audit_count(&pool, "request.answer").await, 1);
        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "queued");
        let author: Value = sqlx::query_scalar(
            "SELECT author FROM task_messages WHERE task_id = $1 ORDER BY queued_at DESC LIMIT 1",
        )
        .bind(task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(author["reason"], "request.decided");
        let again = decide(
            &pool,
            &types(),
            q.id,
            &human(),
            &[],
            Decision::Answer("16".into()),
        )
        .await
        .unwrap_err();
        assert!(matches!(again, RequestError::AlreadyDecided));
    }

    #[tokio::test]
    async fn concurrent_decisions_store_exactly_one() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let (q, _) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &[],
        )
        .await
        .unwrap();
        let t = types();
        let (p1, p2) = (human(), human());
        let (a, b) = tokio::join!(
            decide(&pool, &t, q.id, &p1, &[], Decision::Answer("a".into())),
            decide(&pool, &t, q.id, &p2, &[], Decision::Answer("b".into())),
        );
        assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(), 1);
    }

    #[tokio::test]
    async fn approval_applies_the_action_and_is_audited() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let (r, _) = raise_approval(
            &pool,
            &types(),
            raise_by(task.id, &agent(&task), false),
            "fake",
            json!({"what": "x"}),
            "needed",
        )
        .await
        .unwrap();
        let err = decide(
            &pool,
            &types(),
            r.id,
            &human(),
            &[],
            Decision::Approve(Scope::Instance),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            RequestError::ScopeNotOffered(Scope::Instance)
        ));
        let decided = decide(
            &pool,
            &types(),
            r.id,
            &human(),
            &["board-member".into()],
            Decision::Approve(Scope::Task),
        )
        .await
        .unwrap();
        assert_eq!(decided.state, RequestState::Approved);
        assert_eq!(decided.scope(), Some(Scope::Task));
        assert_eq!(audit_count(&pool, "request.approve").await, 1);
        assert_eq!(audit_count(&pool, "fake.applied").await, 1);
    }

    #[tokio::test]
    async fn failing_action_rolls_the_decision_back() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let (r, _) = raise_approval(
            &pool,
            &types(),
            raise_by(task.id, &agent(&task), false),
            "fake",
            json!({"what": "fail"}),
            "r",
        )
        .await
        .unwrap();
        let err = decide(
            &pool,
            &types(),
            r.id,
            &human(),
            &[],
            Decision::Approve(Scope::Task),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RequestError::ApplyFailed(_)));
        assert_eq!(
            get_request(&pool, r.id).await.unwrap().unwrap().state,
            RequestState::Pending
        );
        let denied = decide(
            &pool,
            &types(),
            r.id,
            &human(),
            &[],
            Decision::Deny(Some("no".into())),
        )
        .await
        .unwrap();
        assert_eq!(denied.comment(), Some("no"));
    }

    #[tokio::test]
    async fn closing_a_task_closes_its_pending_requests() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &[],
        )
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let closed = close_pending_for_task(
            &mut tx,
            task.id,
            "task-completed",
            &Principal::system("task.completed"),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(closed.len(), 1);
        let row = get_request(&pool, closed[0]).await.unwrap().unwrap();
        assert_eq!(
            (row.state, row.close_reason.as_deref()),
            (RequestState::Closed, Some("task-completed"))
        );
        assert_eq!(pending_count(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn marking_the_task_done_closes_pending_requests_for_good() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let (q, _) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &[],
        )
        .await
        .unwrap();
        crate::mark_task_done(&pool, task.id, human())
            .await
            .unwrap();
        let row = get_request(&pool, q.id).await.unwrap().unwrap();
        assert_eq!(
            (row.state, row.close_reason.as_deref()),
            (RequestState::Closed, Some("task-completed"))
        );
        assert!(matches!(
            decide(
                &pool,
                &types(),
                q.id,
                &human(),
                &[],
                Decision::Answer("x".into())
            )
            .await,
            Err(RequestError::AlreadyDecided)
        ));
        let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
            .bind(task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[tokio::test]
    async fn suggested_answers_are_validated() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        for bad in [
            vec!["  ".to_string()],
            vec!["x".repeat(MAX_SUGGESTED_ANSWER_CHARS + 1)],
        ] {
            let err = raise_question(
                &pool,
                raise_by(task.id, &agent(&task), false),
                "Which?",
                &bad,
            )
            .await
            .unwrap_err();
            assert!(matches!(err, RequestError::Invalid(_)), "{err:?}");
        }
        assert_eq!(pending_count(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn padded_suggested_answers_are_stored_trimmed() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task(&pool).await;
        let padded = format!("  {}  ", "x".repeat(MAX_SUGGESTED_ANSWER_CHARS));
        let (q, _) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &[padded],
        )
        .await
        .unwrap();
        assert_eq!(
            q.suggested_answers(),
            ["x".repeat(MAX_SUGGESTED_ANSWER_CHARS)]
        );
    }

    #[tokio::test]
    async fn request_changed_reaches_other_replicas() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let notifier = crate::Notifier::start(&pool).await.unwrap();
        let mut rx = notifier.subscribe();
        let task = task(&pool).await;
        let (q, _) = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &[],
        )
        .await
        .unwrap();
        loop {
            let n = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .unwrap()
                .unwrap();
            if n == (Notification::RequestChanged {
                task_id: task.id,
                request_id: q.id,
            }) {
                break;
            }
        }
    }
}
