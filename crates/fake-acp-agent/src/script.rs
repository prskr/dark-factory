//! Script definition and parsing for `fake-acp-agent`.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

/// Top-level script describing the turns for a fake agent.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Script {
    pub sessions: Sessions,
}

impl<'de> Deserialize<'de> for Script {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ScriptFormat {
            Wrapped { sessions: Sessions },
            SessionsDirect(Sessions),
            TurnsOnly(Vec<Turn>),
        }

        match ScriptFormat::deserialize(deserializer)? {
            ScriptFormat::Wrapped { sessions } | ScriptFormat::SessionsDirect(sessions) => {
                Ok(Script { sessions })
            }
            ScriptFormat::TurnsOnly(new) => Ok(Script {
                sessions: Sessions {
                    new,
                    loaded: Vec::new(),
                },
            }),
        }
    }
}

/// Scripted sessions divided into first-run (`new`) and resumed (`loaded`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Sessions {
    #[serde(default)]
    pub new: Vec<Turn>,
    #[serde(default)]
    pub loaded: Vec<Turn>,
}

/// A prompt turn comprising a sequence of steps.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Turn {
    pub steps: Vec<Step>,
}

impl<'de> Deserialize<'de> for Turn {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum TurnFormat {
            StepsList(Vec<Step>),
            Wrapped { steps: Vec<Step> },
        }

        match TurnFormat::deserialize(deserializer)? {
            TurnFormat::StepsList(steps) | TurnFormat::Wrapped { steps } => Ok(Turn { steps }),
        }
    }
}

/// Individual step in a turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Say(String),
    Think(String),
    ToolCall(ToolCallStep),
    Shell(String),
    Mcp(McpStep),
    Plan(PlanStep),
    Usage(UsageStep),
    End(EndStep),
}

/// Scripted tool call details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolCallStep {
    #[serde(default = "default_call_id")]
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
}

fn default_call_id() -> String {
    "call_1".to_string()
}

/// Scripted MCP call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpStep {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// Scripted plan details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlanStep {
    #[serde(default)]
    pub entries: Vec<PlanEntryStep>,
}

/// Entry within a scripted plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanEntryStep {
    pub content: String,
    #[serde(default = "default_plan_status")]
    pub status: String,
}

fn default_plan_status() -> String {
    "completed".to_string()
}

/// Scripted usage details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct UsageStep {
    #[serde(default)]
    pub used: Option<u64>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

/// End condition of a turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EndStep {
    EndTurn,
    Cancelled,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Crash,
    Hang,
    RateLimited(RateLimitedStep),
}

/// Rate limit details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RateLimitedStep {
    #[serde(default)]
    pub retry_after_seconds: Option<u64>,
}

impl Script {
    /// Parse a script from YAML text.
    pub fn from_yaml_str(yaml: &str) -> anyhow::Result<Self> {
        let script = serde_saphyr::from_str(yaml)?;
        Ok(script)
    }

    /// Load a script from a file (YAML or JSON).
    pub fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let script = serde_json::from_str(&content)?;
            Ok(script)
        } else {
            Self::from_yaml_str(&content)
        }
    }
}
