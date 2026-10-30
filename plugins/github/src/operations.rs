//! GitHub REST operations the plugin calls, by `operationId` of GitHub's REST API description.
//! `cargo xtask update-github-spec` extracts them, and the host tests reject requests to other
//! operations.

/// One entry per REST endpoint of `api.rs`; the GraphQL mutations have no such description.
const OPERATIONS: [&str; 6] = [
    "pulls/get",             // GET /repos/{owner}/{repo}/pulls/{pull_number}
    "pulls/create",          // POST /repos/{owner}/{repo}/pulls
    "pulls/list",            // GET /repos/{owner}/{repo}/pulls (find-open)
    "pulls/update",          // PATCH /repos/{owner}/{repo}/pulls/{pull_number}
    "issues/get",            // GET /repos/{owner}/{repo}/issues/{issue_number}
    "issues/create-comment", // POST /repos/{owner}/{repo}/issues/{issue_number}/comments
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::OPERATIONS;
    use crate::settings::DEFAULT_API_VERSION;

    const EXTRACT: &str = include_str!("../tests/fixtures/github-openapi.json");

    fn extract() -> Value {
        serde_json::from_str(EXTRACT).expect("the OpenAPI extract is JSON")
    }

    #[test]
    fn openapi_extract_has_exactly_the_operations_the_plugin_calls() {
        let extract = extract();
        let ids: BTreeSet<&str> = extract
            .get("paths")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|paths| paths.values())
            .filter_map(Value::as_object)
            .flat_map(|item| item.values())
            .filter_map(|operation| operation.get("operationId"))
            .filter_map(Value::as_str)
            .collect();

        assert_eq!(
            ids,
            BTreeSet::from(OPERATIONS),
            "the OpenAPI extract is out of date, run `cargo xtask update-github-spec`"
        );
    }

    #[test]
    fn openapi_extract_matches_the_default_api_version() {
        let extract = extract();
        let path = extract
            .pointer("/x-dark-factory-source/path")
            .and_then(Value::as_str);

        assert!(
            path.is_some_and(|path| path.contains(DEFAULT_API_VERSION)),
            "the extract comes from {path:?}, the default API version is {DEFAULT_API_VERSION}: \
             update `DESCRIPTION_PATH` in tools/xtask/src/update_github_spec.rs and run \
             `cargo xtask update-github-spec --commit <sha>`"
        );
    }
}
