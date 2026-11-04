use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use df_proto::gateway::v1::{CheckMode, GitRoute, PushCheck, PushPolicy};

use super::*;
use crate::git::refs::{ObjectId, Reason, RefCommand};
use crate::git::secret_rules::BlobSource;
use crate::git::view::{AddedBlob, AddedCommit, PathChange, PushView, TaskRefs, ViewError};
use crate::upstream::BoxError;
use df_proto::push_reasons::{CHECK_FAILED, PUSH_TOO_LARGE};

const AGENT: &str = "agent@factory.test";
const TASK: &str = "refs/heads/factory/task/DF-7";
const WIP: &str = "refs/heads/factory/wip/DF-7";
static NOT_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The objects of a push: a view per pushed tip and blob contents by id.
#[derive(Default)]
struct FakeObjects {
    /// `None` adds more commits than allowed.
    views: HashMap<String, Option<PushView>>,
    blobs: HashMap<gix::ObjectId, Vec<u8>>,
}

impl FakeObjects {
    fn with(mut self, tip: &ObjectId, view: PushView) -> Self {
        self.views.insert(tip.to_string(), Some(view));
        self
    }

    fn blob(mut self, id: gix::ObjectId, content: &[u8]) -> Self {
        self.blobs.insert(id, content.to_vec());
        self
    }
}

impl BlobSource for FakeObjects {
    fn read_blob(&self, id: gix::ObjectId) -> Result<Vec<u8>, BoxError> {
        self.blobs
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("{id} missing").into())
    }
}

impl PushObjects for FakeObjects {
    fn push_view(
        &self,
        task: TaskRefs<'_>,
        tip: &ObjectId,
        max_commits: usize,
        _: &AtomicBool,
    ) -> Result<PushView, ViewError> {
        assert_eq!((task.task_branch, task.wip_branch), (TASK, WIP));
        match self.views.get(tip.as_str()) {
            Some(Some(view)) => Ok(view.clone()),
            Some(None) => Err(ViewError::TooManyCommits(max_commits)),
            None => Err(ViewError::Git(format!("unknown tip {tip}").into())),
        }
    }
}

fn id(n: u8) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(&[n; 20])
}

fn tip(n: u8) -> ObjectId {
    ObjectId::parse(&id(n).to_string()).unwrap()
}

fn zero() -> ObjectId {
    ObjectId::parse(&"0".repeat(40)).unwrap()
}

fn command(name: &str, new: ObjectId) -> RefCommand {
    RefCommand {
        old: zero(),
        new,
        name: name.to_owned(),
    }
}

fn route() -> GitRoute {
    GitRoute {
        task_branch: TASK.to_owned(),
        wip_branch: WIP.to_owned(),
        commit_email: AGENT.to_owned(),
        ..GitRoute::default()
    }
}

/// The defaults of the spec ("Per-project push policy").
fn default_policy() -> PushPolicy {
    PushPolicy {
        protected_globs: [
            ".forgejo/workflows/**",
            ".github/workflows/**",
            ".gitea/workflows/**",
            "CODEOWNERS",
        ]
        .map(str::to_owned)
        .to_vec(),
        protected_mode: CheckMode::Approval.into(),
        secret_mode: CheckMode::Approval.into(),
        size_mode: CheckMode::Approval.into(),
        max_blob_bytes: 5 << 20,
        linear_history: false,
    }
}

/// A commit by `email` that adds the blob `id(blob)` at every path of `paths`.
fn commit_as(email: &str, n: u8, blob: u8, paths: &[&str]) -> AddedCommit {
    AddedCommit {
        id: id(n),
        parents: vec![id(n - 1)],
        author_email: email.to_owned(),
        committer_email: email.to_owned(),
        changes: paths
            .iter()
            .map(|path| PathChange {
                path: (*path).to_owned(),
                old: None,
                new: Some(id(blob)),
            })
            .collect(),
    }
}

/// A view of one agent commit that adds the blob `id(blob)` of `size` bytes at `paths`.
fn adding(n: u8, blob: u8, size: u64, paths: &[&str]) -> PushView {
    PushView {
        commits: vec![commit_as(AGENT, n, blob, paths)],
        blobs: vec![AddedBlob {
            id: id(blob),
            size,
            paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        }],
    }
}

fn github_token(seed: &str) -> String {
    ["ghp_", &seed.repeat(9)].concat()
}

fn config_py(token: &str) -> Vec<u8> {
    format!("import os\n\nGITHUB_TOKEN = \"{token}\"\n").into_bytes()
}

fn checks() -> PolicyChecks {
    PolicyChecks::new(SecretRules::builtin().unwrap())
}

fn run(
    commands: &[RefCommand],
    policy: Option<&PushPolicy>,
    exceptions: &[String],
    objects: &FakeObjects,
) -> ChecksOutcome {
    let route = route();
    let push = CheckedPush {
        route: &route,
        policy,
        exceptions,
        commands,
        interrupt: &NOT_INTERRUPTED,
    };
    checks().check(&push, objects)
}

fn reason_texts(outcome: &ChecksOutcome) -> Vec<Option<String>> {
    outcome
        .reasons
        .iter()
        .map(|reason| reason.as_ref().map(|reason| reason.as_str().to_owned()))
        .collect()
}

#[test]
fn a_committed_api_key_rejects_the_task_branch_naming_rule_path_and_line() {
    let token = github_token("A1b2");
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 60, &["config.py"]))
        .blob(id(50), &config_py(&token));

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert_eq!(
        reason_texts(&outcome),
        [Some(
            "df:secret-scan config.py:3 (betterleaks.github-pat ghp_********)".to_owned()
        )]
    );
    assert_eq!(outcome.findings.len(), 1);
    let found = &outcome.findings[0];
    assert_eq!(found.r#ref, TASK);
    assert_eq!(found.check(), PushCheck::SecretScan);
    assert_eq!(found.mode(), CheckMode::Approval);
    assert_eq!((found.path.as_str(), found.line), ("config.py", Some(3)));
    assert_eq!(
        found.fingerprint,
        Some(format!("secret:betterleaks.github-pat:{}:3", id(50)))
    );
    assert!(outcome.approval_requested);
    assert!(!format!("{outcome:?}").contains(&token[4..]));
}

#[test]
fn a_key_in_a_binary_blob_rejects_the_push_with_a_secret_scan_reason() {
    let label = ["PRIVATE", "KEY"].join(" ");
    let mut content = vec![0_u8; 16];
    content.extend_from_slice(
        format!(
            "-----BEGIN {label}-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END {label}-----\n"
        )
        .as_bytes(),
    );
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 100, &["keystore.bin"]))
        .blob(id(50), &content);

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert_eq!(
        reason_texts(&outcome),
        [Some(
            "df:secret-scan keystore.bin:1 (betterleaks.private-key ----********)".to_owned()
        )]
    );
    assert!(!format!("{outcome:?}").contains("MIIEvQ"));
}

#[test]
fn the_logs_of_a_rejected_push_never_contain_the_matched_value() {
    let token = github_token("Zq9x");
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 60, &["config.py"]))
        .with(&tip(2), adding(2, 51, 60, &["settings.py"]))
        .blob(id(50), &config_py(&token))
        .blob(id(51), &config_py(&token));
    let (logs, guard) = df_telemetry::testing::capture("trace");

    let outcome = run(
        &[command(TASK, tip(1)), command(WIP, tip(2))],
        Some(&default_policy()),
        &[],
        &objects,
    );
    drop(guard);

    assert_eq!(outcome.reasons.iter().flatten().count(), 2);
    let logged = logs.text();
    assert!(logged.contains("push checks rejected a ref"), "{logged}");
    assert!(!logged.contains(&token[4..]), "{logged}");
}

#[test]
fn an_exception_for_a_protected_path_lets_the_push_through() {
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 10, &[".forgejo/workflows/ci.yml"]))
        .blob(id(50), b"on: push\n");
    let commands = [command(TASK, tip(1))];

    let rejected = run(&commands, Some(&default_policy()), &[], &objects);
    let excepted = run(
        &commands,
        Some(&default_policy()),
        &["path:.forgejo/workflows/ci.yml".to_owned()],
        &objects,
    );

    assert_eq!(
        reason_texts(&rejected),
        [Some(
            "df:protected-path .forgejo/workflows/ci.yml (matches .forgejo/workflows/**)"
                .to_owned()
        )]
    );
    assert!(rejected.approval_requested);
    assert_eq!(
        excepted,
        ChecksOutcome {
            reasons: vec![None],
            ..ChecksOutcome::default()
        }
    );
}

#[test]
fn an_exception_for_one_secret_does_not_cover_another() {
    let objects = FakeObjects::default()
        .with(
            &tip(1),
            PushView {
                commits: vec![commit_as(AGENT, 1, 50, &["config.py"])],
                blobs: vec![
                    AddedBlob {
                        id: id(50),
                        size: 60,
                        paths: vec!["config.py".to_owned()],
                    },
                    AddedBlob {
                        id: id(51),
                        size: 60,
                        paths: vec!["settings.py".to_owned()],
                    },
                ],
            },
        )
        .blob(id(50), &config_py(&github_token("A1b2")))
        .blob(id(51), &config_py(&github_token("C3d4")));
    let excepted = format!("secret:betterleaks.github-pat:{}:3", id(50));

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[excepted],
        &objects,
    );

    let paths: Vec<&str> = outcome.findings.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["settings.py"]);
    assert!(outcome.approval_requested);
    assert!(outcome.reasons[0].is_some());
}

#[test]
fn a_wip_push_gets_only_secret_and_size_checks_as_reject() {
    let token = github_token("A1b2");
    let view = PushView {
        commits: vec![commit_as(
            "peter@example.org",
            1,
            50,
            &[".forgejo/workflows/ci.yml"],
        )],
        blobs: vec![AddedBlob {
            id: id(50),
            size: 60,
            paths: vec![".forgejo/workflows/ci.yml".to_owned()],
        }],
    };
    let objects = FakeObjects::default()
        .with(&tip(1), view)
        .blob(id(50), &config_py(&token));

    let outcome = run(
        &[command(WIP, tip(1))],
        Some(&default_policy()),
        &[format!("secret:betterleaks.github-pat:{}:3", id(50))],
        &objects,
    );

    let checks: Vec<(PushCheck, CheckMode)> = outcome
        .findings
        .iter()
        .map(|f| (f.check(), f.mode()))
        .collect();
    assert_eq!(checks, [(PushCheck::SecretScan, CheckMode::Reject)]);
    assert!(!outcome.approval_requested);
}

#[test]
fn a_rejected_wip_ref_next_to_an_approval_rejection_requests_no_approval() {
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 10, &[".forgejo/workflows/ci.yml"]))
        .with(&tip(2), adding(2, 51, 60, &["notes.txt"]))
        .blob(id(50), b"on: push\n")
        .blob(id(51), &config_py(&github_token("A1b2")));

    let outcome = run(
        &[command(TASK, tip(1)), command(WIP, tip(2))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert!(outcome.reasons.iter().all(Option::is_some));
    assert_eq!(outcome.findings.len(), 2);
    assert!(!outcome.approval_requested);
}

#[test]
fn a_deleted_wip_branch_is_not_checked() {
    let outcome = run(
        &[command(WIP, zero())],
        Some(&default_policy()),
        &[],
        &FakeObjects::default(),
    );

    assert_eq!(
        outcome,
        ChecksOutcome {
            reasons: vec![None],
            ..ChecksOutcome::default()
        }
    );
}

#[test]
fn a_push_without_a_push_policy_is_rejected_as_check_failed() {
    let objects = FakeObjects::default().with(&tip(1), adding(1, 50, 1, &["a.txt"]));

    let outcome = run(
        &[command(TASK, tip(1)), command(WIP, tip(1))],
        None,
        &[],
        &objects,
    );

    assert_eq!(
        outcome.reasons,
        [
            Some(Reason::Check(CHECK_FAILED)),
            Some(Reason::Check(CHECK_FAILED))
        ]
    );
    assert!(!outcome.approval_requested);
}

#[test]
fn a_policy_with_an_invalid_glob_rejects_the_push_as_check_failed() {
    let policy = PushPolicy {
        protected_globs: vec!["deploy/[".to_owned()],
        ..default_policy()
    };

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&policy),
        &[],
        &FakeObjects::default(),
    );

    assert_eq!(outcome.reasons, [Some(Reason::Check(CHECK_FAILED))]);
}

#[test]
fn a_ref_adding_too_many_commits_is_rejected_as_push_too_large() {
    let mut objects = FakeObjects::default();
    objects.views.insert(tip(1).to_string(), None);

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert_eq!(outcome.reasons, [Some(Reason::Check(PUSH_TOO_LARGE))]);
    assert!(!outcome.approval_requested);
}

#[test]
fn a_failing_view_or_an_unreadable_blob_rejects_the_ref_as_check_failed() {
    let objects = FakeObjects::default().with(&tip(1), adding(1, 50, 1, &["a.txt"]));

    let outcome = run(
        &[command(TASK, tip(1)), command(WIP, tip(2))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert_eq!(
        outcome.reasons,
        [
            Some(Reason::Check(CHECK_FAILED)),
            Some(Reason::Check(CHECK_FAILED))
        ]
    );
}

#[test]
fn the_reason_of_a_ref_joins_everything_found_and_is_cut_at_one_kib() {
    let paths: Vec<String> = (0..40)
        .map(|n| format!(".forgejo/workflows/job-{n:02}.yml"))
        .collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 10, &paths))
        .blob(id(50), b"on: push\n");

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    let reason = reason_texts(&outcome)[0].clone().unwrap();
    assert_eq!(reason.len(), 1024);
    assert!(reason.starts_with(
        "df:protected-path .forgejo/workflows/job-00.yml (matches .forgejo/workflows/**); \
         df:protected-path .forgejo/workflows/job-01.yml"
    ));
    assert_eq!(outcome.findings.len(), 40);
}

#[test]
fn findings_without_a_path_name_their_commit() {
    let objects = FakeObjects::default().with(
        &tip(1),
        PushView {
            commits: vec![commit_as("peter@example.org", 1, 50, &[])],
            blobs: Vec::new(),
        },
    );

    let outcome = run(
        &[command(TASK, tip(1))],
        Some(&default_policy()),
        &[],
        &objects,
    );

    assert_eq!(
        reason_texts(&outcome),
        [Some(format!(
            "df:identity (commit {} author peter@example.org, committer peter@example.org)",
            id(1)
        ))]
    );
    assert!(!outcome.approval_requested);
}

#[test]
fn control_characters_in_a_reason_become_spaces() {
    let policy = PushPolicy {
        protected_globs: vec!["{ci.yml,x\ty}".to_owned()],
        ..default_policy()
    };
    let objects = FakeObjects::default()
        .with(&tip(1), adding(1, 50, 1, &["ci.yml"]))
        .blob(id(50), b"x");

    let outcome = run(&[command(TASK, tip(1))], Some(&policy), &[], &objects);

    assert_eq!(
        reason_texts(&outcome),
        [Some(
            "df:protected-path ci.yml (matches {ci.yml,x y})".to_owned()
        )]
    );
}
