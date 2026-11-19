//! Project and task keys (`ProjectKey`, `TaskKey`) and derived Git/Kubernetes names.

use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Prefix of a task branch; the task key follows it.
pub const TASK_BRANCH_PREFIX: &str = "factory/task/";

static PROJECT_KEY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9]{1,9}$").expect("valid project key regex"));

/// Validation errors for [`ProjectKey`] and [`TaskKey`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    #[error("invalid project key `{0}`: must match ^[A-Z][A-Z0-9]{{1,9}}$")]
    InvalidProjectKey(String),
    #[error(
        "invalid task key `{0}`: expected `<PROJECT_KEY>-<number>` or `<PROJECT_KEY>-<number>-CHAT` with number >= 1"
    )]
    InvalidTaskKey(String),
}

/// A validated project key matching `^[A-Z][A-Z0-9]{1,9}$` (2 to 10 characters).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectKey(String);

impl ProjectKey {
    /// Parses and validates a project key.
    pub fn new(raw: impl Into<String>) -> Result<Self, KeyError> {
        let s = raw.into();
        if PROJECT_KEY_REGEX.is_match(&s) {
            Ok(Self(s))
        } else {
            Err(KeyError::InvalidProjectKey(s))
        }
    }

    /// Returns the string slice representation of the project key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProjectKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ProjectKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for ProjectKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProjectKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// Suffix of a chat key.
const CHAT_SUFFIX: &str = "-CHAT";

/// Whether `key` is the key of a chat; `false` for any other string, valid key or not.
pub fn is_chat_key(key: &str) -> bool {
    key.parse::<TaskKey>().is_ok_and(|key| key.is_chat())
}

/// A per-project task key `<PROJECT_KEY>-<n>` where `n >= 1`, or a chat key
/// `<PROJECT_KEY>-<n>-CHAT`. Tasks and chats share the number counter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskKey {
    project: ProjectKey,
    number: u64,
    chat: bool,
}

impl TaskKey {
    /// Creates a new `TaskKey` from a validated [`ProjectKey`] and positive task number (`>= 1`).
    pub fn new(project: ProjectKey, number: u64) -> Result<Self, KeyError> {
        Self::build(project, number, false)
    }

    /// Creates the key of a chat (`<PROJECT_KEY>-<n>-CHAT`).
    pub fn chat(project: ProjectKey, number: u64) -> Result<Self, KeyError> {
        Self::build(project, number, true)
    }

    fn build(project: ProjectKey, number: u64, chat: bool) -> Result<Self, KeyError> {
        if number == 0 {
            return Err(KeyError::InvalidTaskKey(format!("{project}-0")));
        }
        Ok(Self {
            project,
            number,
            chat,
        })
    }

    /// Whether this is the key of a chat.
    pub fn is_chat(&self) -> bool {
        self.chat
    }

    /// Returns the project key portion.
    pub fn project(&self) -> &ProjectKey {
        &self.project
    }

    /// Returns the 1-based task number within the project.
    pub fn number(&self) -> u64 {
        self.number
    }

    /// Returns the task branch name (`factory/task/<KEY>`); `None` for a chat, which has none.
    pub fn task_branch(&self) -> Option<String> {
        (!self.chat).then(|| format!("{TASK_BRANCH_PREFIX}{self}"))
    }

    /// The key a task branch names, inverse of [`Self::task_branch`]; a `refs/heads/` prefix is
    /// accepted. `None` for any other branch, and for a chat key.
    pub fn from_task_branch(branch: &str) -> Option<Self> {
        let branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
        let key: Self = branch.strip_prefix(TASK_BRANCH_PREFIX)?.parse().ok()?;
        (!key.chat).then_some(key)
    }

    /// Returns the WIP snapshot branch name (`factory/wip/<KEY>`); `None` for a chat.
    pub fn wip_branch(&self) -> Option<String> {
        (!self.chat).then(|| format!("factory/wip/{self}"))
    }

    /// Returns the Kubernetes PVC / volume name (`task-<key in lower case>`).
    pub fn volume_name(&self) -> String {
        format!("task-{}", self.to_string().to_ascii_lowercase())
    }
}

impl fmt::Display for TaskKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.project, self.number)?;
        if self.chat {
            f.write_str(CHAT_SUFFIX)?;
        }
        Ok(())
    }
}

impl FromStr for TaskKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s_task, chat) = match s.strip_suffix(CHAT_SUFFIX) {
            Some(task) => (task, true),
            None => (s, false),
        };
        let Some((proj_part, num_part)) = s_task.split_once('-') else {
            return Err(KeyError::InvalidTaskKey(s.to_string()));
        };
        if num_part.is_empty()
            || !num_part.bytes().all(|b| b.is_ascii_digit())
            || (num_part.starts_with('0') && num_part.len() > 1)
        {
            return Err(KeyError::InvalidTaskKey(s.to_string()));
        }
        let project = ProjectKey::from_str(proj_part)
            .map_err(|_invalid| KeyError::InvalidTaskKey(s.to_string()))?;
        let number: u64 = num_part
            .parse()
            .map_err(|_invalid| KeyError::InvalidTaskKey(s.to_string()))?;
        Self::build(project, number, chat)
    }
}

impl Serialize for TaskKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TaskKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_project_keys() {
        for valid in ["DF", "AB", "A1", "PROJECT99", "ABCDEFGHIJ"] {
            let key = ProjectKey::from_str(valid).expect("should be valid project key");
            assert_eq!(key.as_str(), valid);
            assert_eq!(key.to_string(), valid);
        }
    }

    #[test]
    fn invalid_project_keys_are_rejected() {
        // Spec scenario: WHEN a project is saved with key `df` or `D` THEN it is rejected
        for invalid in [
            "",
            "D",
            "df",
            "Df",
            "1A",
            "A-B",
            "A_B",
            "ABCDEFGHIJK", // 11 chars > max 10
        ] {
            let err = ProjectKey::from_str(invalid).unwrap_err();
            assert_eq!(err, KeyError::InvalidProjectKey(invalid.to_string()));
        }
    }

    #[test]
    fn task_key_parse_display_and_derived_names_for_df_1() {
        // Spec scenario: WHEN the names for task `DF-1` are derived
        // THEN they are `factory/task/DF-1`, `factory/wip/DF-1` and `task-df-1`
        let key: TaskKey = "DF-1".parse().expect("DF-1 should parse");
        assert_eq!(key.project().as_str(), "DF");
        assert_eq!(key.number(), 1);
        assert_eq!(key.to_string(), "DF-1");
        assert_eq!(key.task_branch().as_deref(), Some("factory/task/DF-1"));
        assert_eq!(key.wip_branch().as_deref(), Some("factory/wip/DF-1"));
        assert_eq!(key.volume_name(), "task-df-1");

        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"DF-1\"");
        let roundtrip: TaskKey = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, key);
    }

    #[test]
    fn task_key_comes_back_from_its_task_branch_only() {
        let key: TaskKey = "DF-12".parse().unwrap();
        assert_eq!(
            TaskKey::from_task_branch(&key.task_branch().unwrap()),
            Some(key.clone())
        );
        assert_eq!(
            TaskKey::from_task_branch("refs/heads/factory/task/DF-12"),
            Some(key)
        );
        for other in [
            "feature/login",
            "factory/wip/DF-1",
            "factory/task/df-1",
            "factory/task/DF-1/x",
        ] {
            assert_eq!(TaskKey::from_task_branch(other), None, "{other}");
        }
    }

    #[test]
    fn chat_key_parses_and_displays_with_suffix() {
        let key: TaskKey = "DF-12-CHAT".parse().unwrap();
        assert!(key.is_chat());
        assert_eq!(key.number(), 12);
        assert_eq!(key.to_string(), "DF-12-CHAT");
        assert_eq!(key.volume_name(), "task-df-12-chat");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"DF-12-CHAT\"");
        assert_eq!(serde_json::from_str::<TaskKey>(&json).unwrap(), key);
        assert!(!"DF-12".parse::<TaskKey>().unwrap().is_chat());
    }

    #[test]
    fn malformed_chat_suffixes_are_rejected() {
        for invalid in [
            "DF-12-chat",
            "DF-12-CHAT-1",
            "DF-12-CHAT-",
            "DF-0-CHAT",
            "DF-12-CHATS",
        ] {
            assert!(TaskKey::from_str(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn branch_names_are_none_for_a_chat_key() {
        let key: TaskKey = "DF-12-CHAT".parse().unwrap();
        assert_eq!(key.task_branch(), None);
        assert_eq!(key.wip_branch(), None);
        assert_eq!(TaskKey::from_task_branch("factory/task/DF-12-CHAT"), None);
    }

    #[test]
    fn invalid_task_keys_are_rejected() {
        for invalid in [
            "DF", "DF-0", "DF-01", "DF-+1", "DF--1", "df-1", "D-1", "DF-", "DF-1-2", "DF-abc",
        ] {
            assert!(
                TaskKey::from_str(invalid).is_err(),
                "expected {invalid} to be rejected"
            );
        }
    }
}
