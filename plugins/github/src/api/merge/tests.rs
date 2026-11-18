//! Scenarios of the `github-integration` spec "Merge a pull request" and "Create a branch".

use serde_json::{Value, json};

use super::super::tests::{calls, fake, github, pr_ref, reply, sent_json};
use super::super::{Method, Response};
use super::*;

fn pr(state: &str, merged: bool) -> Value {
    json!({
        "number": 7, "node_id": "PR_kwDO7", "html_url": "https://github.com/o/r/pull/7",
        "title": "t", "body": "", "state": state, "draft": false,
        "merged_at": if merged { json!("2026-11-18T10:00:00Z") } else { Value::Null },
        "head": {"ref": "factory/task/DF-31", "sha": "abc", "repo": {"full_name": "o/r"}},
        "base": {"ref": "factory/task/DF-30", "sha": "def"},
    })
}

fn flags(merge: bool, squash: bool, rebase: bool) -> Response {
    reply(
        200,
        json!({"allow_merge_commit": merge, "allow_squash_merge": squash, "allow_rebase_merge": rebase}),
    )
}

fn merged_ok() -> Response {
    reply(
        200,
        json!({"merged": true, "sha": "m1", "message": "Pull Request successfully merged"}),
    )
}

fn merge_method(flags: Response) -> anyhow::Result<Value> {
    let fake = fake(vec![reply(200, pr("open", false)), flags, merged_ok()]);
    github(&fake, &[]).merge(&pr_ref(), "abc")?;
    Ok(sent_json(&fake, 2))
}

#[test]
fn merge_pins_the_head_and_prefers_a_merge_commit() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, pr("open", false)),
        flags(true, true, true),
        merged_ok(),
    ]);
    github(&fake, &[]).merge(&pr_ref(), "abc")?;
    assert_eq!(
        calls(&fake),
        [
            (
                Method::Get,
                "https://api.github.com/repos/o/r/pulls/7".to_string()
            ),
            (Method::Get, "https://api.github.com/repos/o/r".to_string()),
            (
                Method::Put,
                "https://api.github.com/repos/o/r/pulls/7/merge".to_string()
            ),
        ]
    );
    assert_eq!(
        sent_json(&fake, 2),
        json!({"sha": "abc", "merge_method": "merge"})
    );
    Ok(())
}

#[test]
fn merge_falls_back_to_squash_then_rebase() -> anyhow::Result<()> {
    assert_eq!(
        merge_method(flags(false, true, true))?["merge_method"],
        "squash"
    );
    assert_eq!(
        merge_method(flags(false, false, true))?["merge_method"],
        "rebase"
    );
    Ok(())
}

#[test]
fn merge_without_any_allowed_method_is_invalid() {
    let fake = fake(vec![
        reply(200, pr("open", false)),
        flags(false, false, false),
    ]);
    let err = github(&fake, &[]).merge(&pr_ref(), "abc").unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn merge_with_a_moved_head_fails_without_asking_the_forge_to_merge() {
    let fake = fake(vec![reply(200, pr("open", false))]);
    let err = github(&fake, &[]).merge(&pr_ref(), "zzz").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m.contains("abc") && m.contains("zzz")));
    assert_eq!(calls(&fake).len(), 1);
}

#[test]
fn merging_an_already_merged_pull_request_with_the_same_head_succeeds() -> anyhow::Result<()> {
    let fake = fake(vec![reply(200, pr("closed", true))]);
    github(&fake, &[]).merge(&pr_ref(), "abc")?;
    assert_eq!(calls(&fake).len(), 1);
    Ok(())
}

#[test]
fn merging_a_pull_request_merged_with_another_head_is_a_conflict() {
    let fake = fake(vec![reply(200, pr("closed", true))]);
    let err = github(&fake, &[]).merge(&pr_ref(), "zzz").unwrap_err();
    assert!(matches!(err, Error::Conflict(_)));
}

#[test]
fn merging_a_closed_pull_request_is_invalid() {
    let fake = fake(vec![reply(200, pr("closed", false))]);
    let err = github(&fake, &[]).merge(&pr_ref(), "abc").unwrap_err();
    assert!(matches!(err, Error::InvalidInput(m) if m.contains("closed")));
}

#[test]
fn a_pull_request_that_is_not_mergeable_is_a_conflict_with_githubs_message() {
    let fake = fake(vec![
        reply(200, pr("open", false)),
        flags(true, true, true),
        reply(405, json!({"message": "Pull Request is not mergeable"})),
    ]);
    let err = github(&fake, &[]).merge(&pr_ref(), "abc").unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m == "Pull Request is not mergeable"));
}

#[test]
fn create_branch_points_the_new_ref_at_the_tip_of_the_origin() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(
            200,
            json!({"ref": "refs/heads/main", "object": {"sha": "s1"}}),
        ),
        reply(201, json!({"ref": "refs/heads/factory/task/DF-30"})),
    ]);
    github(&fake, &[]).create_branch(&pr_ref().repo, "factory/task/DF-30", "main")?;
    assert_eq!(
        calls(&fake),
        [
            (
                Method::Get,
                "https://api.github.com/repos/o/r/git/ref/heads/main".to_string()
            ),
            (
                Method::Post,
                "https://api.github.com/repos/o/r/git/refs".to_string()
            ),
        ]
    );
    assert_eq!(
        sent_json(&fake, 1),
        json!({"ref": "refs/heads/factory/task/DF-30", "sha": "s1"})
    );
    Ok(())
}

#[test]
fn the_origin_branch_keeps_its_slashes_in_the_lookup() -> anyhow::Result<()> {
    let fake = fake(vec![
        reply(200, json!({"object": {"sha": "s1"}})),
        reply(201, json!({})),
    ]);
    github(&fake, &[]).create_branch(&pr_ref().repo, "factory/task/DF-31", "factory/task/DF-30")?;
    assert!(
        calls(&fake)[0]
            .1
            .ends_with("/git/ref/heads/factory/task/DF-30")
    );
    Ok(())
}

#[test]
fn creating_an_existing_branch_is_a_conflict() {
    let fake = fake(vec![
        reply(200, json!({"object": {"sha": "s1"}})),
        reply(422, json!({"message": "Reference already exists"})),
    ]);
    let err = github(&fake, &[])
        .create_branch(&pr_ref().repo, "factory/task/DF-30", "main")
        .unwrap_err();
    assert!(matches!(err, Error::Conflict(m) if m.contains("Reference already exists")));
}

#[test]
fn creating_a_branch_from_an_unknown_ref_is_not_found() {
    let fake = fake(vec![reply(404, json!({"message": "Not Found"}))]);
    let err = github(&fake, &[])
        .create_branch(&pr_ref().repo, "factory/task/DF-30", "nope")
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)));
}
