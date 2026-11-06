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
use df_core::system_reason::REQUEST_DECIDED;
use df_core::{Principal, RequestId, RunId, TaskId};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::catalog::Actor;
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
    /// The type refuses the request for good; the message tells the agent why.
    #[error("{0}")]
    Refused(String),
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
    /// Rejects a payload that does not fit the type or the task; returns it in the form it is
    /// stored in.
    fn validate(&self, task: &TaskRow, payload: &Value) -> Result<Value, RequestError>;
    /// Identical pending requests of a task (same fingerprint) are merged.
    fn fingerprint(&self, payload: &Value) -> String;
    /// The scopes a human may choose from, narrowest first.
    fn scopes(&self) -> &'static [Scope];
    /// Takes the locks `apply` needs before the decision locks the request's task, so lock order
    /// stays the same as on the type's other write paths. Must be re-entrant within a transaction:
    /// `record_approval_in` takes it again when settling runs under it.
    async fn lock(&self, _tx: &mut Transaction<'_, Postgres>) -> Result<(), DbError> {
        Ok(())
    }
    /// Whether what `payload` asks for is granted to the task already, read under [`Self::lock`]
    /// and the task lock. Such a request is not stored.
    async fn granted(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _task: &TaskRow,
        _payload: &Value,
    ) -> Result<bool, RequestError> {
        Ok(false)
    }
    /// A reason the type never accepts what `payload` asks for, read under [`Self::lock`] and the
    /// task lock. The request is rejected with [`RequestError::Refused`] and not stored.
    async fn refused(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _payload: &Value,
    ) -> Result<Option<String>, RequestError> {
        Ok(None)
    }
    /// The action an approval takes, in the decision's transaction.
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        scope: Scope,
        decider: Actor<'_>,
    ) -> Result<(), RequestError>;
    /// Text for the UI card and the decision message.
    fn describe(&self, request: &RequestRow) -> String;
}

/// The approval types of this instance.
#[derive(Clone, Default)]
pub struct ApprovalTypes(BTreeMap<&'static str, Arc<dyn ApprovalType>>);

impl ApprovalTypes {
    /// The approval types the control plane offers.
    pub fn builtin() -> Self {
        Self::default()
            .with(Arc::new(crate::network_access::NetworkAccess))
            .with(Arc::new(crate::push_checks::PushCheckException))
    }

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

/// What [`raise_approval`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum Raised {
    /// The request and whether it is `new`; an identical pending request is returned instead of
    /// a new one.
    Request { request: Box<RequestRow>, new: bool },
    /// What the request asks for is granted already; nothing is stored.
    AlreadyGranted,
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
    let mut tx = pool.begin().await?;
    let raised = raise_in(
        &mut tx,
        NewRequest {
            from,
            kind: RequestKind::Question,
            approval_type: None,
            payload,
            reason: "",
            fingerprint,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(raised)
}

/// Raises an approval request of a registered type on the task, unless the type finds it granted
/// already. An identical pending request is returned with `new` set to false.
///
/// Returns [`RequestError::UnknownType`] for an unregistered type, [`RequestError::NotFound`]
/// for a missing task, and [`RequestError::TooManyPending`] when a new request exceeds the
/// task's pending limit. Type validation and database errors are propagated.
pub async fn raise_approval(
    pool: &PgPool,
    types: &ApprovalTypes,
    from: Raise<'_>,
    type_name: &str,
    payload: Value,
    reason: &str,
) -> Result<Raised, RequestError> {
    let approval_type = types
        .get(type_name)
        .ok_or_else(|| RequestError::UnknownType(type_name.to_string()))?;
    let task = get_task(pool, from.task_id)
        .await?
        .ok_or(RequestError::NotFound)?;
    let payload = approval_type.validate(&task, &payload)?;
    let fingerprint = approval_type.fingerprint(&payload);
    let mut tx = pool.begin().await?;
    // Same lock order as deciding: the type's locks, then the task.
    approval_type.lock(&mut tx).await?;
    lock_task(&mut tx, from.task_id).await?;
    if let Some(reason) = approval_type.refused(&mut tx, &payload).await? {
        return Err(RequestError::Refused(reason));
    }
    if approval_type.granted(&mut tx, &task, &payload).await? {
        return Ok(Raised::AlreadyGranted);
    }
    let (request, new) = raise_in(
        &mut tx,
        NewRequest {
            from,
            kind: RequestKind::Approval,
            approval_type: Some(type_name.to_string()),
            payload,
            reason,
            fingerprint,
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Raised::Request {
        request: Box::new(request),
        new,
    })
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
    if matches!(decision, Decision::Approve(_)) {
        let type_name: Option<String> = sqlx::query_scalar!(
            "SELECT approval_type FROM agent_requests WHERE id = $1",
            request_id.as_uuid()
        )
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        if let Some(approval_type) = type_name.as_deref().and_then(|name| types.get(name)) {
            approval_type.lock(&mut tx).await?;
        }
    }
    let decider = Actor {
        principal: decider,
        roles,
    };
    let (task_id, request) = lock_request(&mut tx, request_id)
        .await?
        .ok_or(RequestError::NotFound)?;
    if request.state != RequestState::Pending {
        return Err(RequestError::AlreadyDecided);
    }
    let (state, stored, action) =
        apply_decision(&mut tx, types, &request, &decision, decider).await?;
    let approval_type = request
        .approval_type
        .as_deref()
        .and_then(|name| types.get(name));
    let decided = record_decision(
        &mut tx,
        task_id,
        request_id,
        decider,
        (state, stored, action),
        approval_type.map(AsRef::as_ref),
    )
    .await?;
    tx.commit().await?;
    Ok(decided)
}

/// Records the approval with `scope` of the request `request_id` of `approval_type`, whose
/// approved action the caller took already, in the caller's transaction. Does nothing if the
/// request is no longer pending.
pub(crate) async fn record_approval_in(
    tx: &mut Transaction<'_, Postgres>,
    approval_type: &dyn ApprovalType,
    request_id: RequestId,
    decider: Actor<'_>,
    scope: Scope,
) -> Result<Option<RequestRow>, DbError> {
    // Re-entrant: the caller may hold the type's lock already.
    approval_type.lock(tx).await?;
    let Some((task_id, request)) = lock_request(tx, request_id).await? else {
        return Ok(None);
    };
    if request.state != RequestState::Pending {
        return Ok(None);
    }
    let decided = record_decision(
        tx,
        task_id,
        request_id,
        decider,
        approved(scope),
        Some(approval_type),
    )
    .await?;
    Ok(Some(decided))
}

/// Denies the pending request `request_id` in the caller's transaction, which holds the approval
/// type's locks already. [`RequestError::NotFound`] and [`RequestError::AlreadyDecided`] if there
/// is no such pending request.
pub(crate) async fn deny_in(
    tx: &mut Transaction<'_, Postgres>,
    approval_type: Option<&dyn ApprovalType>,
    request_id: RequestId,
    decider: Actor<'_>,
    comment: Option<&str>,
) -> Result<RequestRow, RequestError> {
    let (task_id, request) = lock_request(tx, request_id)
        .await?
        .ok_or(RequestError::NotFound)?;
    if request.state != RequestState::Pending {
        return Err(RequestError::AlreadyDecided);
    }
    Ok(record_decision(
        tx,
        task_id,
        request_id,
        decider,
        denied(comment),
        approval_type,
    )
    .await?)
}

/// Stores a decision (state, stored decision, audit action) of the locked request `request_id`,
/// audits it and tells the agent.
async fn record_decision(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    request_id: RequestId,
    decider: Actor<'_>,
    (state, stored, action): (RequestState, Value, &'static str),
    approval_type: Option<&dyn ApprovalType>,
) -> Result<RequestRow, DbError> {
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
    .bind(serde_json::to_value(decider.principal)?)
    .fetch_one(&mut **tx)
    .await?;
    let decided: RequestRow = record.try_into()?;

    let entry = NewAuditEntry::new(
        decider.principal.clone(),
        decider.roles.to_vec(),
        action,
        "request",
        request_id.to_string(),
    )
    .with_details(&json!({ "task_id": task_id, "decision": stored }))?;
    write_audit_entry(&mut **tx, &entry).await?;

    let message = decision_message(approval_type, &decided);
    queue_message_tx(
        tx,
        task_id,
        Principal::system(REQUEST_DECIDED),
        vec![crate::tasks::text_block(message, true)],
    )
    .await?;
    publish(
        &mut **tx,
        &Notification::RequestChanged {
            task_id,
            request_id,
        },
    )
    .await?;
    Ok(decided)
}

/// Locks the task and then the request (like `raise` and `close_pending_for_task`) and returns
/// them; `None` if there is no such request.
async fn lock_request(
    tx: &mut Transaction<'_, Postgres>,
    request_id: RequestId,
) -> Result<Option<(TaskId, RequestRow)>, DbError> {
    let Some(task_uuid): Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT task_id FROM agent_requests WHERE id = $1")
            .bind(request_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?
    else {
        return Ok(None);
    };
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
    Ok(Some((task_id, record.try_into()?)))
}

/// Validates `decision` against the kind of `request` and applies an approval's effect as
/// `decider`. Returns the new state, the stored decision and the audit action.
async fn apply_decision(
    tx: &mut Transaction<'_, Postgres>,
    types: &ApprovalTypes,
    request: &RequestRow,
    decision: &Decision,
    decider: Actor<'_>,
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
            approval_type.apply(tx, request, *scope, decider).await?;
            approved(*scope)
        }
        (Decision::Deny(comment), RequestKind::Approval) => denied(comment.as_deref()),
        _ => {
            return Err(RequestError::Invalid(
                "the decision does not fit the kind of the request".into(),
            ));
        }
    })
}

/// State, stored decision and audit action of a denial with an optional `comment`.
fn denied(comment: Option<&str>) -> (RequestState, Value, &'static str) {
    let comment = comment.map(str::trim).filter(|c| !c.is_empty());
    (
        RequestState::Denied,
        json!({ "comment": comment }),
        "request.deny",
    )
}

/// State, stored decision and audit action of an approval with `scope`.
fn approved(scope: Scope) -> (RequestState, Value, &'static str) {
    (
        RequestState::Approved,
        json!({ "scope": scope.as_str() }),
        "request.approve",
    )
}

/// Closes the pending requests of a task (for example when it completes). Runs in the caller's
/// transaction; the notifications go out on commit. Returns the closed ids.
pub async fn close_pending_for_task(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    reason: &str,
    actor: &Principal,
) -> Result<Vec<RequestId>, DbError> {
    close_pending(tx, task_id, None, reason, actor).await
}

/// [`close_pending_for_task`] for the pending approvals of type `approval_type` only.
pub(crate) async fn close_pending_of_type(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    approval_type: &str,
    reason: &str,
    actor: &Principal,
) -> Result<Vec<RequestId>, DbError> {
    close_pending(tx, task_id, Some(approval_type), reason, actor).await
}

async fn close_pending(
    tx: &mut Transaction<'_, Postgres>,
    task_id: TaskId,
    approval_type: Option<&str>,
    reason: &str,
    actor: &Principal,
) -> Result<Vec<RequestId>, DbError> {
    lock_task(tx, task_id).await?;
    let ids: Vec<uuid::Uuid> = sqlx::query_scalar(
        "UPDATE agent_requests SET state = 'closed', close_reason = $2
         WHERE task_id = $1 AND state = 'pending'
           AND ($3::text IS NULL OR approval_type = $3)
         RETURNING id",
    )
    .bind(task_id.as_uuid())
    .bind(reason)
    .bind(approval_type)
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

/// The text of a decision message to the agent; `approval_type` is the type of an approval.
pub fn decision_message(approval_type: Option<&dyn ApprovalType>, request: &RequestRow) -> String {
    let subject = match request.kind {
        RequestKind::Question => format!("your question \"{}\"", request.question_text()),
        RequestKind::Approval => approval_type.map_or_else(
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

pub(crate) struct NewRequest<'a> {
    pub(crate) from: Raise<'a>,
    pub(crate) kind: RequestKind,
    pub(crate) approval_type: Option<String>,
    pub(crate) payload: Value,
    pub(crate) reason: &'a str,
    pub(crate) fingerprint: String,
}

/// Stores `new` in the caller's transaction, or returns the identical pending request (`false`).
pub(crate) async fn raise_in(
    tx: &mut Transaction<'_, Postgres>,
    new: NewRequest<'_>,
) -> Result<(RequestRow, bool), RequestError> {
    lock_task(tx, new.from.task_id).await?;

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
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(existing) = existing {
        return Ok((existing.try_into()?, false));
    }

    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_requests WHERE task_id = $1 AND state = 'pending'",
    )
    .bind(new.from.task_id.as_uuid())
    .fetch_one(&mut **tx)
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
    .fetch_one(&mut **tx)
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
    write_audit_entry(&mut **tx, &entry).await?;
    publish(
        &mut **tx,
        &Notification::RequestChanged {
            task_id: new.from.task_id,
            request_id: id,
        },
    )
    .await?;
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

pub(crate) fn text_block(text: String) -> ContentBlock {
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
mod tests;
