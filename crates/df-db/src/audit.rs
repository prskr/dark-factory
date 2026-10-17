//! Append-only audit log writer (`specs/audit-log/spec.md`).
//!
//! Audit details reference credentials by name only and never contain secret values.
//! Note that [`secrecy::SecretString`] does not implement [`serde::Serialize`], so any
//! struct containing a `SecretString` is rejected at compile time when passed to
//! [`AuditEntry::with_details`].

use chrono::{DateTime, Utc};
use df_core::{CredentialRef, Principal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgExecutor;

use crate::pool::DbError;

/// Non-secret metadata recorded when a credential reference is created or updated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRefAuditDetails {
    pub name: String,
    pub secret_name: String,
    pub secret_key: String,
}

impl From<&CredentialRef> for CredentialRefAuditDetails {
    fn from(r: &CredentialRef) -> Self {
        Self {
            name: r.name.clone(),
            secret_name: r.secret_name.clone(),
            secret_key: r.secret_key.clone(),
        }
    }
}

/// An append-only audit entry to be persisted in `audit_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAuditEntry {
    pub actor: Principal,
    pub roles: Vec<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: Value,
}

impl NewAuditEntry {
    /// Creates a new audit entry with empty JSON object `{}` details.
    pub fn new(
        actor: Principal,
        roles: impl Into<Vec<String>>,
        action: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            actor,
            roles: roles.into(),
            action: action.into(),
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            details: serde_json::json!({}),
        }
    }

    /// Attaches structured details to this audit entry.
    ///
    /// Because [`secrecy::SecretString`] does not implement [`serde::Serialize`], a type
    /// holding a `SecretString` cannot be passed here.
    pub fn with_details<T: Serialize>(mut self, details: &T) -> Result<Self, DbError> {
        self.details = serde_json::to_value(details)?;
        Ok(self)
    }
}

/// Stored audit entry row from `audit_log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLogRow {
    pub id: i64,
    pub occurred_at: DateTime<Utc>,
    pub actor: Principal,
    pub roles: Vec<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub details: Value,
}

/// Appends an entry to `audit_log` and returns the persisted row.
pub async fn write_audit_entry<'e, E>(
    executor: E,
    entry: &NewAuditEntry,
) -> Result<AuditLogRow, DbError>
where
    E: PgExecutor<'e>,
{
    let actor_json = serde_json::to_value(&entry.actor)?;
    let row = sqlx::query!(
        r#"
        INSERT INTO audit_log (actor, roles, action, resource_type, resource_id, details)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, occurred_at, actor, roles, action, resource_type, resource_id, details
        "#,
        actor_json,
        &entry.roles,
        entry.action,
        entry.resource_type,
        entry.resource_id,
        entry.details
    )
    .fetch_one(executor)
    .await?;

    let actor: Principal = serde_json::from_value(row.actor)?;
    Ok(AuditLogRow {
        id: row.id,
        occurred_at: row.occurred_at,
        actor,
        roles: row.roles,
        action: row.action,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        details: row.details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_core::{CredentialRefId, PersonaId, RunId, TaskId, UserId};
    use sqlx::PgPool;

    /// Compile-time check that `secrecy::SecretString` does NOT implement `serde::Serialize`,
    /// guaranteeing it can never be passed into `NewAuditEntry::with_details`.
    #[test]
    fn secret_string_cannot_be_serialized_into_audit_details() {
        static_assertions::assert_not_impl_any!(secrecy::SecretString: Serialize);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn human_task_creation_and_agent_tool_call_and_credential_ref_are_audited(pool: PgPool) {
        // Scenario 1: Task creation is audited
        let peter = Principal::human(UserId::new(), "Peter");
        let entry1 = NewAuditEntry::new(
            peter.clone(),
            vec!["board-member".to_string()],
            "task.create",
            "task",
            "DF-1",
        )
        .with_details(&serde_json::json!({ "title": "Fix login" }))
        .unwrap();

        let saved1 = write_audit_entry(&pool, &entry1).await.unwrap();
        assert_eq!(saved1.actor, peter);
        assert_eq!(saved1.roles, vec!["board-member"]);
        assert_eq!(saved1.action, "task.create");
        assert_eq!(saved1.resource_id, "DF-1");

        // Scenario 2: Agent tool call is audited
        let agent = Principal::agent(PersonaId::new(), "Linus", Some(RunId::new()), TaskId::new());
        let entry2 = NewAuditEntry::new(
            agent.clone(),
            Vec::<String>::new(),
            "tool.comment_on_issue",
            "issue",
            "42",
        )
        .with_details(&serde_json::json!({ "issue_number": 42 }))
        .unwrap();

        let saved2 = write_audit_entry(&pool, &entry2).await.unwrap();
        assert_eq!(saved2.actor, agent);
        assert_eq!(saved2.action, "tool.comment_on_issue");

        // Scenario 3: Credential reference edited — records name, Secret name and key, no secret value
        let cred_ref = CredentialRef {
            id: CredentialRefId::new(),
            name: "claude-sub".into(),
            secret_name: "k8s-claude-secret".into(),
            secret_key: "oauth-token".into(),
        };
        let cred_details = CredentialRefAuditDetails::from(&cred_ref);
        let entry3 = NewAuditEntry::new(
            peter,
            vec!["board-member".to_string()],
            "credential_ref.create",
            "credential_ref",
            &cred_ref.name,
        )
        .with_details(&cred_details)
        .unwrap();

        let saved3 = write_audit_entry(&pool, &entry3).await.unwrap();
        assert_eq!(
            saved3.details,
            serde_json::json!({
                "name": "claude-sub",
                "secret_name": "k8s-claude-secret",
                "secret_key": "oauth-token"
            })
        );
    }
}
