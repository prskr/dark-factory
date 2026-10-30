//! `cargo xtask update-github-spec`: a minimal extract of GitHub's REST API description.
//!
//! Keeps the operations named in `plugins/github/src/operations.rs`, the parameters of their
//! path items and the components they reference transitively, without documentation keywords.
//! The pin (commit, path, sha256 of the description) is recorded in `x-dark-factory-source`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "github/rest-api-description";
/// Keep in sync with the plugin's default `api-version` (`plugins/github/src/settings.rs`).
const DESCRIPTION_PATH: &str = "descriptions-next/api.github.com/api.github.com.2026-03-10.json";
const FIXTURE_PATH: &str = "plugins/github/tests/fixtures/github-openapi.json";
const OPERATIONS_PATH: &str = "plugins/github/src/operations.rs";
const SOURCE_KEY: &str = "x-dark-factory-source";
/// The description has about 14 MB; ureq reads at most 10 MB unless told otherwise.
const MAX_DESCRIPTION_BYTES: u64 = 64 * 1024 * 1024;

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "patch", "delete", "head", "options", "trace",
];
/// Keywords that only document; keys starting with `x-` are dropped as well.
const DOC_KEYWORDS: [&str; 6] = [
    "description",
    "summary",
    "externalDocs",
    "examples",
    "example",
    "tags",
];
const COMPONENTS_PREFIX: &str = "#/components/";

/// What `update-github-spec` does with the extract.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Mode<'a> {
    /// Downloads the recorded pin again and writes the extract.
    Update,
    /// Like `Update`, but fails instead of writing when the committed extract differs.
    Check,
    /// Pins this commit (a full SHA) with [`DESCRIPTION_PATH`] and writes the extract.
    Commit(&'a str),
}

/// Runs `update-github-spec` in `mode` for the workspace at `root`.
pub(crate) fn run(root: &Path, mode: Mode<'_>) -> Result<()> {
    update(root, mode, download)
}

/// Where the description of an extract comes from, written to `x-dark-factory-source`.
#[derive(Debug, Clone, Serialize)]
struct Source {
    repository: String,
    commit: String,
    path: String,
    sha256: String,
}

/// What to download, and the sha256 recorded for it (`None` for a new pin).
struct Pin {
    commit: String,
    path: String,
    sha256: Option<String>,
}

fn update(root: &Path, mode: Mode<'_>, fetch: impl FnOnce(&str) -> Result<Vec<u8>>) -> Result<()> {
    let fixture = root.join(FIXTURE_PATH);
    let committed = read_if_exists(&fixture)?;
    let pin = pin(committed.as_deref(), mode)?;
    let url = description_url(&pin.commit, &pin.path);
    let bytes = fetch(&url)?;
    let source = verified_source(pin, &bytes)?;
    let description: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("{url} is not JSON"))?;
    let operations = fs::read_to_string(root.join(OPERATIONS_PATH))
        .with_context(|| format!("reading {OPERATIONS_PATH}"))?;
    let operations = operations_from_source(&operations)?;
    let rendered = render(&extract(&description, &operations, source)?)?;
    if matches!(mode, Mode::Check) {
        if committed.as_deref() != Some(rendered.as_str()) {
            bail!("{FIXTURE_PATH} is stale; run `cargo xtask update-github-spec`");
        }
        return Ok(());
    }
    if let Some(dir) = fixture.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&fixture, rendered).with_context(|| format!("writing {FIXTURE_PATH}"))
}

fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// [`Mode::Commit`] with [`DESCRIPTION_PATH`], else the pin recorded in the committed extract.
fn pin(committed: Option<&str>, mode: Mode<'_>) -> Result<Pin> {
    if let Mode::Commit(commit) = mode {
        if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("--commit {commit}: expected a full 40-character commit SHA");
        }
        return Ok(Pin {
            commit: commit.to_ascii_lowercase(),
            path: DESCRIPTION_PATH.into(),
            sha256: None,
        });
    }
    let committed =
        committed.with_context(|| format!("{FIXTURE_PATH} does not exist; pass --commit <sha>"))?;
    let extract: Value =
        serde_json::from_str(committed).with_context(|| format!("{FIXTURE_PATH} is not JSON"))?;
    let recorded = |key: &str| {
        extract
            .get(SOURCE_KEY)
            .and_then(|source| source.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("{FIXTURE_PATH} has no `{SOURCE_KEY}.{key}`"))
    };
    Ok(Pin {
        commit: recorded("commit")?,
        path: recorded("path")?,
        sha256: Some(recorded("sha256")?),
    })
}

/// The source of `description`; fails when its sha256 differs from the recorded one.
fn verified_source(pin: Pin, description: &[u8]) -> Result<Source> {
    let sha256 = sha256_hex(description);
    if let Some(recorded) = pin.sha256
        && recorded != sha256
    {
        bail!(
            "{} at {} has sha256 {sha256}, {FIXTURE_PATH} records {recorded}: upstream changed the \
             pinned file",
            pin.path,
            pin.commit
        );
    }
    Ok(Source {
        repository: REPOSITORY.into(),
        commit: pin.commit,
        path: pin.path,
        sha256,
    })
}

#[expect(
    clippy::format_collect,
    reason = "32 bytes: readability over one allocation per byte"
)]
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The raw URL of the description at `path` in [`REPOSITORY`] at `commit`.
fn description_url(commit: &str, path: &str) -> String {
    format!("https://raw.githubusercontent.com/{REPOSITORY}/{commit}/{path}")
}

fn download(url: &str) -> Result<Vec<u8>> {
    crate::get(url)?
        .body_mut()
        .with_config()
        .limit(MAX_DESCRIPTION_BYTES)
        .read_to_vec()
        .with_context(|| format!("reading {url}"))
}

/// The string literals of the `OPERATIONS` array in the plugin's `operations.rs`.
fn operations_from_source(source: &str) -> Result<Vec<String>> {
    let (_, rest) = source
        .split_once("const OPERATIONS")
        .context("no `const OPERATIONS` array")?;
    let (array, _) = rest
        .split_once("];")
        .context("the `OPERATIONS` array is not terminated by `];`")?;
    let operations: Vec<String> = array
        .lines()
        .flat_map(|line| {
            let code = line.split("//").next().unwrap_or_default();
            code.split('"').skip(1).step_by(2)
        })
        .map(str::to_owned)
        .collect();
    if operations.is_empty() {
        bail!("the `OPERATIONS` array names no operation");
    }
    Ok(operations)
}

/// The paths with `operations` and the components they reference, without documentation.
fn extract(description: &Value, operations: &[String], source: Source) -> Result<Value> {
    let paths = description
        .get("paths")
        .and_then(Value::as_object)
        .context("the description has no `paths`")?;
    let wanted: BTreeSet<&str> = operations.iter().map(String::as_str).collect();
    let mut found = BTreeSet::new();
    let mut kept = Map::new();
    for (path, item) in paths {
        if let Some(item) = kept_operations(item, &wanted, &mut found) {
            kept.insert(path.clone(), item);
        }
    }
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|id| !found.contains(*id))
        .collect();
    if !missing.is_empty() {
        bail!(
            "operations missing from the description: {}",
            missing.join(", ")
        );
    }

    let paths = strip_docs(Value::Object(kept));
    let components = referenced_components(description, &paths)?;
    let mut extract = Map::new();
    extract.insert("openapi".into(), field(description, "/openapi")?);
    let mut info = Map::new();
    info.insert("title".into(), field(description, "/info/title")?);
    info.insert("version".into(), field(description, "/info/version")?);
    extract.insert("info".into(), Value::Object(info));
    extract.insert(SOURCE_KEY.into(), serde_json::to_value(source)?);
    extract.insert("paths".into(), paths);
    if !components.is_empty() {
        let components = components
            .into_iter()
            .map(|(kind, named)| (kind, Value::Object(named)))
            .collect();
        extract.insert("components".into(), Value::Object(components));
    }
    Ok(Value::Object(extract))
}

/// Pretty-printed `extract` with sorted keys and a trailing newline.
fn render(extract: &Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(extract)?;
    text.push('\n');
    Ok(text)
}

/// The operations of a path item named in `wanted` plus the item's `parameters`, if any is named.
fn kept_operations(
    item: &Value,
    wanted: &BTreeSet<&str>,
    found: &mut BTreeSet<String>,
) -> Option<Value> {
    let item = item.as_object()?;
    let mut kept = Map::new();
    for (method, operation) in item {
        let Some(id) = operation.get("operationId").and_then(Value::as_str) else {
            continue;
        };
        if HTTP_METHODS.contains(&method.as_str()) && wanted.contains(id) {
            found.insert(id.to_owned());
            kept.insert(method.clone(), operation.clone());
        }
    }
    if kept.is_empty() {
        return None;
    }
    if let Some(parameters) = item.get("parameters") {
        kept.insert("parameters".into(), parameters.clone());
    }
    Some(Value::Object(kept))
}

fn field(description: &Value, pointer: &str) -> Result<Value> {
    description
        .pointer(pointer)
        .cloned()
        .with_context(|| format!("the description has no `{pointer}`"))
}

/// `value` without documentation keywords; property names are kept.
fn strip_docs(value: Value) -> Value {
    strip(value, false)
}

/// `keys_are_property_names`: the keys of `value` name properties, they are not keywords.
fn strip(value: Value, keys_are_property_names: bool) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| keys_are_property_names || !is_doc_keyword(key))
                .map(|(key, value)| {
                    let names_properties = !keys_are_property_names
                        && (key == "properties" || key == "patternProperties");
                    let value = strip(value, names_properties);
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(strip_docs).collect()),
        other => other,
    }
}

fn is_doc_keyword(key: &str) -> bool {
    DOC_KEYWORDS.contains(&key) || key.starts_with("x-")
}

/// Every component referenced from `roots`, transitively, by kind and name.
fn referenced_components(
    description: &Value,
    roots: &Value,
) -> Result<BTreeMap<String, Map<String, Value>>> {
    let mut components: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut pending = Vec::new();
    collect_refs(roots, &mut pending);
    while let Some(reference) = pending.pop() {
        let (kind, name) = component_ref(&reference)?;
        if components
            .get(&kind)
            .is_some_and(|named| named.contains_key(&name))
        {
            continue;
        }
        let component = description
            .get("components")
            .and_then(|all| all.get(&kind))
            .and_then(|named| named.get(&name))
            .with_context(|| format!("`$ref` {reference} points to a missing component"))?;
        let component = strip_docs(component.clone());
        collect_refs(&component, &mut pending);
        components.entry(kind).or_default().insert(name, component);
    }
    Ok(components)
}

fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value) {
                    ("$ref", Value::String(reference)) => refs.push(reference.clone()),
                    _ => collect_refs(value, refs),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_refs(item, refs)),
        _ => {}
    }
}

/// Kind and decoded name of a `#/components/<kind>/<name>` reference.
fn component_ref(reference: &str) -> Result<(String, String)> {
    let parts = reference
        .strip_prefix(COMPONENTS_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .filter(|(kind, name)| !kind.is_empty() && !name.is_empty() && !name.contains('/'));
    let Some((kind, name)) = parts else {
        bail!("unsupported `$ref` {reference}: expected `{COMPONENTS_PREFIX}<kind>/<name>`");
    };
    Ok((unescape(kind), unescape(name)))
}

/// Decodes a JSON pointer token (RFC 6901: `~1` before `~0`).
fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const COMMIT: &str = "141149d417b34fb386d6767e47c69b3fdfef2097";

    fn ops(ids: &[&str]) -> Vec<String> {
        ids.iter().map(ToString::to_string).collect()
    }

    fn source() -> Source {
        Source {
            repository: REPOSITORY.into(),
            commit: "abc".into(),
            path: DESCRIPTION_PATH.into(),
            sha256: "def".into(),
        }
    }

    fn spec(paths: Value, components: Value) -> Value {
        json!({
            "openapi": "3.1.0",
            "info": {"title": "GitHub v3 REST API", "version": "1.1.4", "description": "doc"},
            "paths": paths,
            "components": components,
        })
    }

    fn get(operation_id: &str, rest: Value) -> Value {
        let mut operation = json!({"operationId": operation_id});
        if let (Some(operation), Value::Object(rest)) = (operation.as_object_mut(), rest) {
            operation.extend(rest);
        }
        json!({ "get": operation })
    }

    fn workspace(operations: &[&str]) -> Result<tempfile::TempDir> {
        let root = tempfile::tempdir()?;
        write_operations(root.path(), operations)?;
        Ok(root)
    }

    fn write_operations(root: &Path, operations: &[&str]) -> Result<()> {
        let dir = root.join("plugins/github/src");
        fs::create_dir_all(&dir)?;
        let literals: Vec<String> = operations.iter().map(|id| format!("\"{id}\",")).collect();
        let source = format!(
            "pub const OPERATIONS: [&str; {}] = [\n{}\n];\n",
            operations.len(),
            literals.join("\n")
        );
        fs::write(dir.join("operations.rs"), source)?;
        Ok(())
    }

    fn committed(root: &tempfile::TempDir) -> Result<String> {
        Ok(fs::read_to_string(root.path().join(FIXTURE_PATH))?)
    }

    fn description() -> Vec<u8> {
        let paths = json!({"/a": get("pulls/get", json!({})), "/b": get("issues/get", json!({}))});
        spec(paths, json!({})).to_string().into_bytes()
    }

    #[test]
    fn operations_from_source_reads_the_literals_and_ignores_comments() -> Result<()> {
        let source = r#"
            /// Not `"doc/comment"`.
            pub const OPERATIONS: [&str; 2] = [
                "pulls/get", // GET "/pulls"
                // "commented/out",
                "issues/get",
            ];
            const OTHER: &str = "other/const";
        "#;
        assert_eq!(
            operations_from_source(source)?,
            ops(&["pulls/get", "issues/get"])
        );
        Ok(())
    }

    #[test]
    fn operations_from_source_reads_the_plugin_operations() -> Result<()> {
        let source = include_str!("../../../plugins/github/src/operations.rs");

        let operations = operations_from_source(source)?;

        assert!(
            operations.contains(&"pulls/get".to_owned()),
            "{operations:?}"
        );
        Ok(())
    }

    #[test]
    fn operations_from_source_rejects_a_missing_empty_or_unterminated_array() {
        assert!(operations_from_source("const OTHER: [&str; 1] = [\"a\"];").is_err());
        assert!(operations_from_source("const OPERATIONS: [&str; 0] = [];").is_err());
        assert!(operations_from_source("const OPERATIONS: [&str; 1] = [\n\"a\",\n").is_err());
    }

    #[test]
    fn extract_keeps_only_the_named_operations_and_their_path_parameters() -> Result<()> {
        let description = spec(
            json!({
                "/repos/{owner}/{repo}/pulls/{pull_number}": {
                    "parameters": [{"name": "pull_number", "in": "path"}],
                    "get": {"operationId": "pulls/get"},
                    "put": {"operationId": "pulls/merge"},
                },
                "/user": {
                    "parameters": [{"name": "since", "in": "query"}],
                    "get": {"operationId": "users/get-authenticated"},
                },
            }),
            json!({}),
        );

        let extract = extract(&description, &ops(&["pulls/get"]), source())?;

        assert_eq!(
            extract,
            json!({
                "openapi": "3.1.0",
                "info": {"title": "GitHub v3 REST API", "version": "1.1.4"},
                "x-dark-factory-source": {
                    "repository": REPOSITORY,
                    "commit": "abc",
                    "path": DESCRIPTION_PATH,
                    "sha256": "def",
                },
                "paths": {
                    "/repos/{owner}/{repo}/pulls/{pull_number}": {
                        "parameters": [{"name": "pull_number", "in": "path"}],
                        "get": {"operationId": "pulls/get"},
                    },
                },
            })
        );
        Ok(())
    }

    #[test]
    fn extract_names_every_missing_operation() {
        let description = spec(json!({"/a": get("pulls/get", json!({}))}), json!({}));

        let err = extract(
            &description,
            &ops(&["pulls/get", "pulls/update", "issues/get"]),
            source(),
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("issues/get, pulls/update"),
            "{err}"
        );
    }

    #[test]
    fn extract_copies_the_components_referenced_transitively() -> Result<()> {
        let description = spec(
            json!({"/a": get("pulls/get", json!({
                "parameters": [{"$ref": "#/components/parameters/owner"}],
                "responses": {"200": {"content": {"application/json": {
                    "schema": {"$ref": "#/components/schemas/pull-request"},
                }}}},
            }))}),
            json!({
                "parameters": {"owner": {"name": "owner", "in": "path"}},
                "responses": {"unused": {"content": {}}},
                "schemas": {
                    "pull-request": {"properties": {"user": {"$ref": "#/components/schemas/user"}}},
                    "user": {"properties": {"id": {"$ref": "#/components/schemas/id"}}},
                    "id": {"type": "integer"},
                    "unused": {"type": "string"},
                },
            }),
        );

        let extract = extract(&description, &ops(&["pulls/get"]), source())?;

        assert_eq!(
            extract.get("components"),
            Some(&json!({
                "parameters": {"owner": {"name": "owner", "in": "path"}},
                "schemas": {
                    "pull-request": {"properties": {"user": {"$ref": "#/components/schemas/user"}}},
                    "user": {"properties": {"id": {"$ref": "#/components/schemas/id"}}},
                    "id": {"type": "integer"},
                },
            }))
        );
        Ok(())
    }

    #[test]
    fn extract_decodes_escaped_component_names() -> Result<()> {
        let description = spec(
            json!({"/a": get("pulls/get", json!({
                "requestBody": {"$ref": "#/components/requestBodies/a~1b~0c"},
            }))}),
            json!({"requestBodies": {"a/b~c": {"required": true}}}),
        );

        let extract = extract(&description, &ops(&["pulls/get"]), source())?;

        assert_eq!(
            extract.get("components"),
            Some(&json!({"requestBodies": {"a/b~c": {"required": true}}}))
        );
        Ok(())
    }

    #[test]
    fn extract_rejects_a_ref_to_a_missing_component() {
        let description = spec(
            json!({"/a": get("pulls/get", json!({
                "requestBody": {"$ref": "#/components/requestBodies/missing"},
            }))}),
            json!({"requestBodies": {}}),
        );

        let err = extract(&description, &ops(&["pulls/get"]), source()).unwrap_err();

        assert!(
            err.to_string()
                .contains("#/components/requestBodies/missing"),
            "{err}"
        );
    }

    #[test]
    fn extract_rejects_a_ref_outside_the_components() {
        for reference in [
            "#/paths/~1b",
            "other.json#/components/schemas/a",
            "#/components/schemas",
        ] {
            let description = spec(
                json!({"/a": get("pulls/get", json!({"requestBody": {"$ref": reference}}))}),
                json!({}),
            );

            let err = extract(&description, &ops(&["pulls/get"]), source()).unwrap_err();

            assert!(err.to_string().contains(reference), "{err}");
        }
    }

    #[test]
    fn extract_strips_documentation_keywords_but_keeps_property_names() -> Result<()> {
        let description = spec(
            json!({"/a": get("pulls/get", json!({
                "summary": "Get a pull request",
                "description": "Long text",
                "tags": ["pulls"],
                "externalDocs": {"url": "https://docs.github.com"},
                "x-github": {"category": "pulls"},
                "requestBody": {"$ref": "#/components/requestBodies/body"},
            }))}),
            json!({"requestBodies": {"body": {"content": {"application/json": {"schema": {
                "type": "object",
                "description": "doc",
                "x-github-breaking-changes": [],
                "properties": {
                    "description": {"type": "string", "description": "the body"},
                    "title": {"type": "string", "example": "Fix"},
                },
                "patternProperties": {"x-.*": {"type": "string", "examples": ["a"]}},
            }}}}}}),
        );

        let extract = extract(&description, &ops(&["pulls/get"]), source())?;

        assert_eq!(
            extract.pointer("/paths/~1a/get"),
            Some(&json!({
                "operationId": "pulls/get",
                "requestBody": {"$ref": "#/components/requestBodies/body"},
            }))
        );
        assert_eq!(
            extract.pointer("/components/requestBodies/body/content/application~1json/schema"),
            Some(&json!({
                "type": "object",
                "properties": {
                    "description": {"type": "string"},
                    "title": {"type": "string"},
                },
                "patternProperties": {"x-.*": {"type": "string"}},
            }))
        );
        Ok(())
    }

    #[test]
    fn extract_drops_components_only_referenced_from_examples() -> Result<()> {
        let description = spec(
            json!({"/a": get("pulls/get", json!({
                "responses": {"200": {"content": {"application/json": {
                    "schema": {"type": "object"},
                    "examples": {"default": {"$ref": "#/components/examples/pull-request"}},
                }}}},
            }))}),
            json!({"examples": {"pull-request": {"value": {}}}}),
        );

        let extract = extract(&description, &ops(&["pulls/get"]), source())?;

        assert_eq!(extract.get("components"), None);
        Ok(())
    }

    #[test]
    fn render_sorts_the_keys_and_ends_with_a_newline() -> Result<()> {
        let mut inner = Map::new();
        inner.insert("d".into(), json!(2));
        inner.insert("c".into(), json!(3));
        let mut outer = Map::new();
        outer.insert("b".into(), json!(1));
        outer.insert("a".into(), Value::Object(inner));

        assert_eq!(
            render(&Value::Object(outer))?,
            "{\n  \"a\": {\n    \"c\": 3,\n    \"d\": 2\n  },\n  \"b\": 1\n}\n"
        );
        Ok(())
    }

    #[test]
    fn update_with_a_commit_writes_the_extract_and_records_the_pin() -> Result<()> {
        let root = workspace(&["pulls/get"])?;
        let mut urls = Vec::new();

        update(root.path(), Mode::Commit(COMMIT), |url| {
            urls.push(url.to_owned());
            Ok(description())
        })?;

        assert_eq!(urls, [description_url(COMMIT, DESCRIPTION_PATH)]);
        let written: Value = serde_json::from_str(&committed(&root)?)?;
        assert_eq!(
            written.get(SOURCE_KEY),
            Some(&json!({
                "repository": REPOSITORY,
                "commit": COMMIT,
                "path": DESCRIPTION_PATH,
                "sha256": sha256_hex(&description()),
            }))
        );
        assert_eq!(
            written.pointer("/paths/~1a/get/operationId"),
            Some(&json!("pulls/get"))
        );
        assert_eq!(written.pointer("/paths/~1b"), None);
        Ok(())
    }

    #[test]
    fn update_without_a_commit_downloads_the_recorded_pin_again() -> Result<()> {
        let root = workspace(&["pulls/get"])?;
        update(root.path(), Mode::Commit(COMMIT), |_| Ok(description()))?;
        let before = committed(&root)?;
        let mut urls = Vec::new();

        update(root.path(), Mode::Update, |url| {
            urls.push(url.to_owned());
            Ok(description())
        })?;

        assert_eq!(urls, [description_url(COMMIT, DESCRIPTION_PATH)]);
        assert_eq!(committed(&root)?, before);
        Ok(())
    }

    #[test]
    fn update_fails_when_the_pinned_file_changed_upstream() -> Result<()> {
        let root = workspace(&["pulls/get"])?;
        update(root.path(), Mode::Commit(COMMIT), |_| Ok(description()))?;
        let before = committed(&root)?;
        let mut changed = description();
        changed.push(b'\n');

        let err = update(root.path(), Mode::Update, |_| Ok(changed)).unwrap_err();

        assert!(err.to_string().contains("upstream changed"), "{err:#}");
        assert_eq!(committed(&root)?, before);
        Ok(())
    }

    #[test]
    fn update_without_a_committed_extract_requires_a_commit() -> Result<()> {
        let root = workspace(&["pulls/get"])?;

        for mode in [Mode::Update, Mode::Check] {
            let err = update(root.path(), mode, |_| panic!("nothing is pinned")).unwrap_err();
            assert!(err.to_string().contains("--commit"), "{err:#}");
        }
        Ok(())
    }

    #[test]
    fn update_rejects_a_commit_that_is_not_a_full_sha() -> Result<()> {
        let root = workspace(&["pulls/get"])?;

        for commit in [
            "main",
            "141149d",
            "../../141149d417b34fb386d6767e47c69b3fdfef20",
        ] {
            let err =
                update(root.path(), Mode::Commit(commit), |_| panic!("invalid pin")).unwrap_err();
            assert!(err.to_string().contains(commit), "{err:#}");
        }
        Ok(())
    }

    #[test]
    fn check_fails_for_a_stale_extract_and_names_the_command() -> Result<()> {
        let root = workspace(&["pulls/get"])?;
        update(root.path(), Mode::Commit(COMMIT), |_| Ok(description()))?;
        update(root.path(), Mode::Check, |_| Ok(description()))?;
        write_operations(root.path(), &["pulls/get", "issues/get"])?;
        let before = committed(&root)?;

        let err = update(root.path(), Mode::Check, |_| Ok(description())).unwrap_err();

        assert!(
            err.to_string().contains("cargo xtask update-github-spec"),
            "{err:#}"
        );
        assert_eq!(committed(&root)?, before);
        Ok(())
    }

    #[test]
    fn sha256_hex_is_the_lowercase_hex_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
