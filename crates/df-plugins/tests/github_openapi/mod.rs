//! Checks the GitHub mock traffic against the extract of GitHub's REST description
//! (`plugins/github/tests/fixtures/github-openapi.json`).
//!
//! A schema is validated with the extract's `components` as the root document, so its
//! `#/components/...` references resolve without a remote lookup.

use anyhow::{Context, Result, anyhow, bail, ensure};
use jsonschema::Draft;
use serde_json::{Value, json};

const EXTRACT: &str = include_str!("../../../../plugins/github/tests/fixtures/github-openapi.json");

/// Validates a request to the mock GitHub, `path` relative to the route's `apiUrl`, `query` the
/// decoded query pairs; returns the `operationId`. `/graphql` has no description: skip it.
pub(crate) fn validate_request(
    method: &str,
    path: &str,
    query: &[(String, String)],
    body: Option<&Value>,
) -> Result<String> {
    check_request(&extract()?, method, path, query, body)
}

/// Validates a mocked response body against the operation's response schema for `status`.
///
/// The mocks carry only the fields the plugin reads, so the `required` keyword is removed from
/// every schema first (properties named `required` stay). The fields that are present must
/// still match their type, enum and format.
pub(crate) fn validate_response(operation_id: &str, status: u16, body: &Value) -> Result<()> {
    let mut doc = extract()?;
    strip_required(&mut doc);
    let operation = operations(&doc)
        .find(|operation| operation["operationId"] == operation_id)
        .with_context(|| format!("no operation `{operation_id}` in the OpenAPI extract"))?;
    let response = operation["responses"]
        .get(status.to_string())
        .with_context(|| format!("{operation_id} declares no response {status}"))?;
    let schema = json_schema(resolve(&doc, response)?)?;
    validate(&doc, schema, body).with_context(|| format!("{operation_id} {status}"))
}

/// Validates a request the mock received; see [`validate_request`].
pub(crate) fn validate_received(request: &wiremock::Request) -> Result<String> {
    let method = request.method.as_str();
    let context = || format!("{method} {}", request.url);
    let query: Vec<(String, String)> = request.url.query_pairs().into_owned().collect();
    let body = if request.body.is_empty() {
        None
    } else {
        let body = serde_json::from_slice::<Value>(&request.body)
            .context("the request body is not JSON")
            .with_context(context)?;
        Some(body)
    };
    validate_request(method, request.url.path(), &query, body.as_ref()).with_context(context)
}

fn extract() -> Result<Value> {
    serde_json::from_str(EXTRACT).context("the OpenAPI extract is not JSON")
}

/// [`validate_request`] against the description `doc`.
fn check_request(
    doc: &Value,
    method: &str,
    path: &str,
    query: &[(String, String)],
    body: Option<&Value>,
) -> Result<String> {
    let (template, path_item, operation) = find_operation(doc, method, path)
        .with_context(|| format!("no operation in the OpenAPI extract for {method} {path}"))?;
    let operation_id = operation_id(operation)?;
    let parameters = [path_item, operation]
        .into_iter()
        .filter_map(|item| item.get("parameters").and_then(Value::as_array))
        .flatten()
        .map(|parameter| resolve(doc, parameter))
        .collect::<Result<Vec<_>>>()?;
    let path_values = template
        .split('/')
        .zip(path.split('/'))
        .filter_map(|(t, p)| {
            let name = t.strip_prefix('{')?.strip_suffix('}')?;
            Some((name, p))
        });
    for (name, raw) in path_values {
        check_parameter(doc, &parameters, "path", name, raw)
            .with_context(|| format!("{operation_id}: path parameter `{name}`"))?;
    }
    for (name, raw) in query {
        check_parameter(doc, &parameters, "query", name, raw)
            .with_context(|| format!("{operation_id}: query parameter `{name}`"))?;
    }
    let missing = parameters
        .iter()
        .filter(|p| p["in"] == "query" && p["required"] == true)
        .filter_map(|p| p["name"].as_str())
        .find(|name| !query.iter().any(|(n, _)| n == name));
    if let Some(name) = missing {
        bail!("{operation_id}: required query parameter `{name}` is missing");
    }
    let request_body = operation
        .get("requestBody")
        .map(|request_body| resolve(doc, request_body))
        .transpose()?;
    match (request_body, body) {
        (None, Some(_)) => bail!("{operation_id} takes no request body"),
        (Some(rb), None) if rb["required"] == true => {
            bail!("{operation_id} requires a request body")
        }
        (Some(rb), Some(body)) => validate(doc, json_schema(rb)?, body)
            .with_context(|| format!("{operation_id}: request body"))?,
        _ => {}
    }
    Ok(operation_id)
}

/// Validates `raw` against the schema of the parameter `name` the operation declares `in`.
fn check_parameter(
    doc: &Value,
    parameters: &[&Value],
    location: &str,
    name: &str,
    raw: &str,
) -> Result<()> {
    let parameter = parameters
        .iter()
        .find(|p| p["in"] == location && p["name"] == name)
        .context("not declared by the operation")?;
    let schema = &parameter["schema"];
    validate(doc, schema, &parameter_value(schema, raw))
}

/// Every operation of the extract.
fn operations(doc: &Value) -> impl Iterator<Item = &Value> {
    doc["paths"]
        .as_object()
        .into_iter()
        .flat_map(|paths| paths.values())
        .filter_map(Value::as_object)
        .flat_map(|item| item.values())
        .filter(|operation| operation.get("operationId").is_some())
}

/// The path template, path item and operation for `method` whose template has the shape of
/// `path`; the caller validates the path parameters against their schemas.
fn find_operation<'a>(
    doc: &'a Value,
    method: &str,
    path: &str,
) -> Option<(&'a str, &'a Value, &'a Value)> {
    let (template, path_item) = doc["paths"]
        .as_object()?
        .iter()
        .find(|(template, _)| template_matches(template, path))?;
    let operation = path_item.get(method.to_ascii_lowercase())?;
    Some((template.as_str(), path_item, operation))
}

/// A `{name}` segment of `template` matches one non-empty segment of `path`.
fn template_matches(template: &str, path: &str) -> bool {
    let template: Vec<&str> = template.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    template.len() == path.len()
        && template.iter().zip(&path).all(|(t, p)| {
            if t.starts_with('{') && t.ends_with('}') {
                !p.is_empty()
            } else {
                t == p
            }
        })
}

fn operation_id(operation: &Value) -> Result<String> {
    operation["operationId"]
        .as_str()
        .map(str::to_string)
        .context("an operation without operationId")
}

/// Follows `$ref`s (`#/<JSON pointer>`) within the extract.
fn resolve<'a>(doc: &'a Value, mut value: &'a Value) -> Result<&'a Value> {
    while let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        value = reference
            .strip_prefix('#')
            .and_then(|pointer| doc.pointer(pointer))
            .with_context(|| format!("unresolvable reference `{reference}`"))?;
    }
    Ok(value)
}

/// The `application/json` schema of a request body or response object.
fn json_schema(item: &Value) -> Result<&Value> {
    item.pointer("/content/application~1json/schema")
        .with_context(|| format!("no application/json schema in {item}"))
}

/// A path segment or query string as the JSON value the parameter's schema expects; unparsable
/// stays a string.
fn parameter_value(schema: &Value, raw: &str) -> Value {
    let parsed = match schema["type"].as_str() {
        Some("integer") => raw.parse::<i64>().ok().map(Value::from),
        Some("number") => raw.parse::<f64>().ok().map(Value::from),
        Some("boolean") => raw.parse::<bool>().ok().map(Value::from),
        _ => None,
    };
    parsed.unwrap_or_else(|| Value::from(raw))
}

fn validate(doc: &Value, schema: &Value, instance: &Value) -> Result<()> {
    let root = json!({
        "$ref": "#/$defs/target",
        "$defs": {"target": schema},
        "components": doc["components"],
    });
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .build(&root)
        .map_err(|e| anyhow!("invalid schema: {e}"))?;
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{} at `{}`", e, e.instance_path()))
        .collect();
    ensure!(errors.is_empty(), "{}", errors.join("; "));
    Ok(())
}

/// Removes the `required` keyword from every schema, but not properties named `required`.
fn strip_required(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("required");
            for (key, child) in map.iter_mut() {
                if key == "properties" || key == "patternProperties" {
                    child
                        .as_object_mut()
                        .into_iter()
                        .flat_map(|properties| properties.values_mut())
                        .for_each(strip_required);
                } else {
                    strip_required(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_required),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn query(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn request_with_a_valid_body_passes_validation() -> Result<()> {
        let body =
            json!({"title": "DF-1", "body": "b", "head": "f", "base": "main", "draft": true});
        assert_eq!(
            validate_request("POST", "/repos/o/r/pulls", &[], Some(&body))?,
            "pulls/create"
        );
        Ok(())
    }

    #[test]
    fn request_with_valid_query_parameters_passes_validation() -> Result<()> {
        let query = query(&[("state", "open"), ("head", "o:f"), ("per_page", "100")]);
        assert_eq!(
            validate_request("GET", "/repos/o/r/pulls", &query, None)?,
            "pulls/list"
        );
        Ok(())
    }

    #[test]
    fn request_with_valid_path_parameters_passes_validation() -> Result<()> {
        assert_eq!(
            validate_request("GET", "/repos/o/r/issues/7", &[], None)?,
            "issues/get"
        );
        Ok(())
    }

    #[test]
    fn path_parameter_with_a_wrong_type_fails_validation() {
        let err = validate_request("GET", "/repos/o/r/pulls/abc", &[], None).unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("pulls/get: path parameter `pull_number`"),
            "{err}"
        );
    }

    #[test]
    fn request_without_a_required_query_parameter_fails_validation() -> Result<()> {
        let doc = json!({
            "paths": {"/search": {"get": {"operationId": "search", "parameters": [
                {"in": "query", "name": "q", "required": true, "schema": {"type": "string"}}
            ]}}},
            "components": {},
        });
        let err = check_request(&doc, "GET", "/search", &[], None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "search: required query parameter `q` is missing"
        );
        let query = query(&[("q", "x")]);
        assert_eq!(
            check_request(&doc, "GET", "/search", &query, None)?,
            "search"
        );
        Ok(())
    }

    #[test]
    fn request_with_an_unknown_query_parameter_fails_validation() {
        let query = query(&[("state", "open"), ("bogus", "1")]);
        let err = validate_request("GET", "/repos/o/r/pulls", &query, None).unwrap_err();
        let err = format!("{err:#}");
        assert!(
            err.contains("query parameter `bogus`: not declared"),
            "{err}"
        );
    }

    #[test]
    fn query_parameter_with_a_wrong_type_fails_validation() {
        let query = query(&[("per_page", "many")]);
        let err = validate_request("GET", "/repos/o/r/pulls", &query, None).unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("query parameter `per_page`"), "{err}");
    }

    #[test]
    fn request_to_an_operation_missing_from_the_extract_fails_validation() {
        for (method, path) in [
            ("DELETE", "/repos/o/r/pulls/1"),
            ("GET", "/repos/o/r/branches"),
        ] {
            let err = validate_request(method, path, &[], None).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("no operation in the OpenAPI extract for {method} {path}")
            );
        }
    }

    #[test]
    fn request_body_with_a_wrong_field_type_fails_validation() {
        let body = json!({"head": "f", "base": "main", "draft": "yes"});
        assert!(validate_request("POST", "/repos/o/r/pulls", &[], Some(&body)).is_err());
    }

    #[test]
    fn request_without_a_required_body_fails_validation() {
        assert!(validate_request("POST", "/repos/o/r/issues/7/comments", &[], None).is_err());
    }

    #[test]
    fn request_with_a_body_the_operation_does_not_take_fails_validation() {
        let body = json!({});
        assert!(validate_request("GET", "/repos/o/r/pulls/7", &[], Some(&body)).is_err());
    }

    #[test]
    fn response_with_a_subset_of_the_fields_passes_validation() -> Result<()> {
        let body = json!({"number": 7, "draft": false, "head": {"ref": "f"}});
        validate_response("pulls/get", 200, &body)
    }

    #[test]
    fn response_with_a_wrong_field_type_fails_validation() {
        let err = validate_response("pulls/get", 200, &json!({"number": "one"})).unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("number"), "{err}");
    }

    #[test]
    fn response_with_a_value_outside_the_enum_fails_validation() {
        let body = json!({"state": "merged"});
        assert!(validate_response("pulls/get", 200, &body).is_err());
    }

    #[test]
    fn response_with_a_malformed_format_fails_validation() {
        let body = json!({"merged_at": "yesterday"});
        assert!(validate_response("pulls/get", 200, &body).is_err());
    }
}
