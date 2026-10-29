//! `UUIDv7` identifier newtypes for Dark Factory domain entities.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generates a new time-ordered `UUIDv7` identifier.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing [`Uuid`].
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// Returns the underlying [`Uuid`].
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

define_id!(
    /// Identifier for a human user (`users.id`).
    UserId
);
define_id!(
    /// Identifier for a credential reference (`credential_refs.id`).
    CredentialRefId
);
define_id!(
    /// Identifier for a project (`projects.id`).
    ProjectId
);
define_id!(
    /// Identifier for an agent persona (`personas.id`).
    PersonaId
);
define_id!(
    /// Identifier for a task (`tasks.id`).
    TaskId
);
define_id!(
    /// Identifier for a queued prompt/message (`task_messages.id`).
    PromptId
);
define_id!(
    /// Identifier for an agent run (`runs.id`).
    RunId
);
define_id!(
    /// Identifier for a factory tool call (`tool_calls.call_id`).
    ToolCallId
);
define_id!(
    /// Identifier for an agent request (`agent_requests.id`).
    RequestId
);
define_id!(
    /// Identifier for an egress allowlist entry (`egress_allowlist.id`).
    EgressEntryId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuidv7_ids_generate_and_roundtrip_string_and_json() {
        let project_id = ProjectId::new();
        let task_id = TaskId::new();
        let run_id = RunId::new();

        assert_eq!(project_id.as_uuid().get_version_num(), 7);
        assert_eq!(task_id.as_uuid().get_version_num(), 7);
        assert_eq!(run_id.as_uuid().get_version_num(), 7);

        let s = project_id.to_string();
        let parsed: ProjectId = s.parse().unwrap();
        assert_eq!(parsed, project_id);

        let json = serde_json::to_string(&task_id).unwrap();
        let decoded: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, task_id);
    }
}
