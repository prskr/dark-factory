//! Parsing and checking of `sandbox.affinity`.
//!
//! The value is a Kubernetes `affinity` kept as JSON in the instance configuration. `k8s-openapi`
//! skips unknown fields silently, so a misspelled field would turn into "no rule"; the parse goes
//! through `serde_ignored` and rejects every ignored field. A few mistakes the parse cannot see
//! are checked on the parsed value. Everything else is left to the API server, which rejects the
//! pod (`provisioning_failed` with its message).

use k8s_openapi::api::core::v1::{
    Affinity, NodeSelectorRequirement, NodeSelectorTerm, PodAffinityTerm,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelectorRequirement;
use thiserror::Error;

const SELECTOR_OPERATORS: [&str; 4] = ["In", "NotIn", "Exists", "DoesNotExist"];
const NODE_SELECTOR_OPERATORS: [&str; 6] = ["In", "NotIn", "Exists", "DoesNotExist", "Gt", "Lt"];

/// `sandbox.affinity` is not a usable Kubernetes affinity.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AffinityError {
    #[error("`sandbox.affinity` has an unknown field `{0}`")]
    UnknownField(String),
    #[error("`sandbox.affinity` is invalid: {0}")]
    WrongType(String),
    #[error("`sandbox.affinity` is invalid at `{path}`: {problem}")]
    Invalid { path: String, problem: String },
}

/// Parses `value` into an [`Affinity`] and checks it.
///
/// # Errors
///
/// `value` has the wrong type somewhere, an unknown field, or fails one of the checks; the error
/// names `sandbox.affinity` and, below the root, the path of the invalid entry; a value of the
/// wrong type names only `sandbox.affinity`.
pub fn parse_affinity(value: &serde_json::Value) -> Result<Affinity, AffinityError> {
    let mut ignored = Vec::new();
    let mut on_ignored = |path: serde_ignored::Path<'_>| ignored.push(path.to_string());
    let tracked = serde_ignored::Deserializer::new(value, &mut on_ignored);
    let affinity: Affinity = serde_path_to_error::deserialize(tracked).map_err(|e| {
        match e.path().to_string().as_str() {
            // The root path prints as `.`: the value itself has the wrong type.
            "." => AffinityError::WrongType(e.inner().to_string()),
            path => invalid(path, e.inner().to_string()),
        }
    })?;
    if let Some(path) = ignored.into_iter().next() {
        // `serde_ignored` marks the `Option` wrappers of the target type with `?`.
        return Err(AffinityError::UnknownField(path.replace("?.", "")));
    }
    check(&affinity)?;
    Ok(affinity)
}

fn invalid(path: &str, problem: impl Into<String>) -> AffinityError {
    AffinityError::Invalid {
        path: path.to_string(),
        problem: problem.into(),
    }
}

fn check(affinity: &Affinity) -> Result<(), AffinityError> {
    if let Some(node) = &affinity.node_affinity {
        let path = "nodeAffinity";
        for (i, p) in node
            .preferred_during_scheduling_ignored_during_execution
            .iter()
            .flatten()
            .enumerate()
        {
            let at = format!("{path}.preferredDuringSchedulingIgnoredDuringExecution[{i}]");
            check_weight(&at, p.weight)?;
            check_node_term(&at, &p.preference)?;
        }
        if let Some(required) = &node.required_during_scheduling_ignored_during_execution {
            for (i, term) in required.node_selector_terms.iter().enumerate() {
                let at = format!(
                    "{path}.requiredDuringSchedulingIgnoredDuringExecution.nodeSelectorTerms[{i}]"
                );
                check_node_term(&at, term)?;
            }
        }
    }
    let pods = [
        (
            "podAffinity",
            affinity.pod_affinity.as_ref().map(|a| {
                (
                    &a.preferred_during_scheduling_ignored_during_execution,
                    &a.required_during_scheduling_ignored_during_execution,
                )
            }),
        ),
        (
            "podAntiAffinity",
            affinity.pod_anti_affinity.as_ref().map(|a| {
                (
                    &a.preferred_during_scheduling_ignored_during_execution,
                    &a.required_during_scheduling_ignored_during_execution,
                )
            }),
        ),
    ];
    for (name, terms) in pods {
        let Some((preferred, required)) = terms else {
            continue;
        };
        for (i, p) in preferred.iter().flatten().enumerate() {
            let at = format!("{name}.preferredDuringSchedulingIgnoredDuringExecution[{i}]");
            check_weight(&at, p.weight)?;
            check_pod_term(&at, &p.pod_affinity_term)?;
        }
        for (i, term) in required.iter().flatten().enumerate() {
            let at = format!("{name}.requiredDuringSchedulingIgnoredDuringExecution[{i}]");
            check_pod_term(&at, term)?;
        }
    }
    Ok(())
}

fn check_weight(path: &str, weight: i32) -> Result<(), AffinityError> {
    if (1..=100).contains(&weight) {
        Ok(())
    } else {
        Err(invalid(path, format!("weight {weight} is outside 1-100")))
    }
}

fn check_pod_term(path: &str, term: &PodAffinityTerm) -> Result<(), AffinityError> {
    if term.topology_key.is_empty() {
        return Err(invalid(path, "`topologyKey` must not be empty"));
    }
    for selector in [&term.label_selector, &term.namespace_selector]
        .into_iter()
        .flatten()
    {
        for r in selector.match_expressions.iter().flatten() {
            check_requirement(path, r)?;
        }
    }
    Ok(())
}

fn check_requirement(path: &str, r: &LabelSelectorRequirement) -> Result<(), AffinityError> {
    check_operator(
        path,
        &r.key,
        &r.operator,
        r.values.as_deref(),
        &SELECTOR_OPERATORS,
    )
}

fn check_node_term(path: &str, term: &NodeSelectorTerm) -> Result<(), AffinityError> {
    for r in term.match_expressions.iter().flatten() {
        check_node_requirement(path, r)?;
    }
    Ok(())
}

fn check_node_requirement(path: &str, r: &NodeSelectorRequirement) -> Result<(), AffinityError> {
    check_operator(
        path,
        &r.key,
        &r.operator,
        r.values.as_deref(),
        &NODE_SELECTOR_OPERATORS,
    )
}

fn check_operator(
    path: &str,
    key: &str,
    operator: &str,
    values: Option<&[String]>,
    known: &[&str],
) -> Result<(), AffinityError> {
    if !known.contains(&operator) {
        return Err(invalid(
            path,
            format!("unknown operator `{operator}` for key `{key}`"),
        ));
    }
    let has_values = values.is_some_and(|v| !v.is_empty());
    match operator {
        "Exists" | "DoesNotExist" if has_values => Err(invalid(
            path,
            format!("operator `{operator}` of key `{key}` takes no `values`"),
        )),
        "Exists" | "DoesNotExist" => Ok(()),
        _ if !has_values => Err(invalid(
            path,
            format!("operator `{operator}` of key `{key}` needs `values`"),
        )),
        _ => Ok(()),
    }
}
