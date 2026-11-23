//! Scheduling and resource settings of the sandbox pod (`sandbox.nodeSelector`,
//! `sandbox.tolerations`, `sandbox.resources`) and their startup validation.
//!
//! These types mirror the small part of the Kubernetes API they need so that `df-core` does not
//! depend on `k8s-openapi`; `df-k8s` maps them onto the pod.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::ConfigError;

static QUANTITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+|[KMGTPE]i|[numkMGTPE])?$")
        .expect("the quantity pattern is a valid regex")
});

/// Match operator of a [`SandboxToleration`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolerationOperator {
    /// The taint's value must equal the toleration's value.
    #[default]
    Equal,
    /// Any value matches.
    Exists,
}

impl TolerationOperator {
    /// The Kubernetes name of the operator.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Equal => "Equal",
            Self::Exists => "Exists",
        }
    }
}

/// Taint effect a [`SandboxToleration`] matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

impl TaintEffect {
    /// The Kubernetes name of the effect.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSchedule => "NoSchedule",
            Self::PreferNoSchedule => "PreferNoSchedule",
            Self::NoExecute => "NoExecute",
        }
    }
}

/// A taint a sandbox pod tolerates (Kubernetes `Toleration`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxToleration {
    /// Taint key; empty with operator `Exists` matches every taint.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub operator: TolerationOperator,
    #[serde(default)]
    pub value: Option<String>,
    /// Taint effect; unset matches all effects.
    #[serde(default)]
    pub effect: Option<TaintEffect>,
    /// How long a `NoExecute` taint is tolerated; unset tolerates it forever.
    #[serde(default)]
    pub toleration_seconds: Option<i64>,
}

/// Requests and limits of the agent container: Kubernetes resource name to quantity.
///
/// No defaults: the chart sets them, the control plane puts exactly these entries on the pod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxResources {
    #[serde(default)]
    pub requests: BTreeMap<String, String>,
    #[serde(default)]
    pub limits: BTreeMap<String, String>,
}

/// Whether `name` is a Kubernetes qualified name: `[prefix/]name`.
pub(super) fn is_qualified_name(name: &str) -> bool {
    let (prefix, local) = match name.split_once('/') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, name),
    };
    prefix.is_none_or(crate::host::is_dns_subdomain) && !local.is_empty() && is_label_value(local)
}

/// Whether `value` is a Kubernetes label value or the name part of a qualified name: at most 63
/// characters, alphanumerics, `-`, `_` and `.`, starting and ending with an alphanumeric. The
/// empty string is valid as a label value; callers that need a name check for emptiness.
fn is_label_value(value: &str) -> bool {
    let edge = |c: char| c.is_ascii_alphanumeric();
    value.len() <= 63
        && (value.is_empty()
            || (value.starts_with(edge)
                && value.ends_with(edge)
                && value
                    .chars()
                    .all(|c| edge(c) || matches!(c, '-' | '_' | '.'))))
}

pub(super) fn validate_node_selector(
    node_selector: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    for (key, value) in node_selector {
        if !is_qualified_name(key) {
            return Err(ConfigError::Invalid(format!(
                "`sandbox.nodeSelector` key {key:?} is not a valid label key: use `[prefix/]name`, \
                 a name of at most 63 alphanumerics, `-`, `_` or `.` that starts and ends with an alphanumeric"
            )));
        }
        if !is_label_value(value) {
            return Err(ConfigError::Invalid(format!(
                "`sandbox.nodeSelector` value {value:?} of key {key:?} is not a valid label value: \
                 use at most 63 alphanumerics, `-`, `_` or `.`, starting and ending with an alphanumeric"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_tolerations(tolerations: &[SandboxToleration]) -> Result<(), ConfigError> {
    for (index, t) in tolerations.iter().enumerate() {
        let key = t.key.as_deref().unwrap_or_default();
        let problem = if !key.is_empty() && !is_qualified_name(key) {
            Some(format!("key {key:?} is not a valid label key"))
        } else if key.is_empty() && t.operator != TolerationOperator::Exists {
            Some("an empty key needs operator `Exists`".to_string())
        } else if t.operator == TolerationOperator::Exists && t.value.is_some() {
            Some("operator `Exists` takes no value".to_string())
        } else if t.toleration_seconds.is_some() && t.effect != Some(TaintEffect::NoExecute) {
            Some("`tolerationSeconds` needs effect `NoExecute`".to_string())
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(ConfigError::Invalid(format!(
                "`sandbox.tolerations` entry {index} is invalid: {problem}"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_resources(resources: &SandboxResources) -> Result<(), ConfigError> {
    for (map, entries) in [
        ("requests", &resources.requests),
        ("limits", &resources.limits),
    ] {
        for (name, quantity) in entries {
            if !is_qualified_name(name) {
                return Err(ConfigError::Invalid(format!(
                    "`sandbox.resources.{map}` entry {name:?} is not a valid resource name"
                )));
            }
            if !QUANTITY.is_match(quantity) {
                return Err(ConfigError::Invalid(format!(
                    "`sandbox.resources.{map}` entry {name:?} has the invalid quantity {quantity:?}: \
                     use a number with an optional suffix such as `250m`, `4Gi` or `1.5`"
                )));
            }
        }
    }
    Ok(())
}
