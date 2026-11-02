use std::collections::HashMap;

use df_proto::gateway::v1::{CheckMode, PushPolicy};

use super::*;
use crate::git::view::{AddedCommit, PathChange, PushView};

const AGENT: &str = "agent@factory.test";
const MIB: u64 = 1024 * 1024;

fn id(n: u8) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(&[n; 20])
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
        max_blob_bytes: 5 * MIB,
        linear_history: false,
    }
}

/// A commit by the agent with parent `id(n - 1)` that adds a blob at each of `paths`.
fn commit(n: u8, paths: &[&str]) -> AddedCommit {
    AddedCommit {
        id: id(n),
        parents: vec![id(n - 1)],
        author_email: AGENT.to_owned(),
        committer_email: AGENT.to_owned(),
        changes: paths
            .iter()
            .map(|path| PathChange {
                path: (*path).to_owned(),
                old: None,
                new: Some(id(n + 100)),
            })
            .collect(),
    }
}

fn blob(n: u8, size: u64, paths: &[&str]) -> AddedBlob {
    AddedBlob {
        id: id(n),
        size,
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
    }
}

fn view(commits: Vec<AddedCommit>, blobs: Vec<AddedBlob>) -> PushView {
    PushView { commits, blobs }
}

/// Answers with fixed matches per blob id; an unknown blob has none.
#[derive(Default)]
struct FakeSecrets {
    matches: HashMap<gix::ObjectId, Vec<SecretMatch>>,
    failing: Option<gix::ObjectId>,
}

impl FakeSecrets {
    fn with(blob: gix::ObjectId, rule: &str, line: u32) -> Self {
        let found = SecretMatch {
            rule: rule.to_owned(),
            line,
            masked: "ghp_********".to_owned(),
        };
        Self {
            matches: HashMap::from([(blob, vec![found])]),
            failing: None,
        }
    }
}

impl SecretMatcher for FakeSecrets {
    fn find(&self, blob: &AddedBlob) -> Result<Vec<SecretMatch>, ScanError> {
        if self.failing == Some(blob.id) {
            return Err(ScanError {
                blob: blob.id,
                source: "unreadable".into(),
            });
        }
        Ok(self.matches.get(&blob.id).cloned().unwrap_or_default())
    }
}

fn check_with(
    view: &PushView,
    policy: &PushPolicy,
    target: Target,
    secrets: &FakeSecrets,
) -> Vec<Finding> {
    let policy = CheckPolicy::new(policy).unwrap();
    check_push(view, &policy, &Identity { email: AGENT }, target, secrets).unwrap()
}

fn check(view: &PushView, policy: &PushPolicy) -> Vec<Finding> {
    check_with(view, policy, Target::TaskBranch, &FakeSecrets::default())
}

fn kinds(findings: &[Finding]) -> Vec<(Check, Mode)> {
    findings
        .iter()
        .map(|finding| (finding.check, finding.mode))
        .collect()
}

fn approval_finding(fingerprint: &str) -> Finding {
    Finding {
        check: Check::ProtectedPath,
        mode: Mode::Approval,
        fingerprint: Some(fingerprint.to_owned()),
        path: String::new(),
        line: None,
        detail: String::new(),
        secret: None,
    }
}

#[test]
fn a_clean_push_has_no_findings_and_is_forwarded() {
    let view = view(
        vec![commit(2, &["src/main.rs"])],
        vec![blob(102, 100, &["src/main.rs"])],
    );

    let findings = check(&view, &default_policy());

    assert_eq!(findings, []);
    assert_eq!(verdict(findings, &[]), Verdict::Forward);
}

#[test]
fn a_commit_as_a_human_is_an_identity_rejection() {
    let mut human = commit(2, &[]);
    human.author_email = "peter@example.org".to_owned();
    let mut committed_by_human = commit(3, &[]);
    committed_by_human.committer_email = "peter@example.org".to_owned();
    let view = view(vec![committed_by_human, human], vec![]);

    let findings = check(&view, &default_policy());

    assert_eq!(
        findings,
        [
            Finding {
                check: Check::Identity,
                mode: Mode::Reject,
                fingerprint: None,
                path: String::new(),
                line: None,
                detail: format!("commit {} committer peter@example.org", id(3)),
                secret: None,
            },
            Finding {
                check: Check::Identity,
                mode: Mode::Reject,
                fingerprint: None,
                path: String::new(),
                line: None,
                detail: format!("commit {} author peter@example.org", id(2)),
                secret: None,
            },
        ]
    );
    assert_eq!(
        PushCheck::from(Check::Identity).reason_code(),
        Some("df:identity")
    );
    assert!(matches!(
        verdict(findings, &[String::new()]),
        Verdict::Reject {
            approval_requested: false,
            ..
        }
    ));
}

#[test]
fn a_changed_workflow_in_approval_mode_requests_approval() {
    let view = view(vec![commit(2, &[".forgejo/workflows/ci.yml"])], vec![]);

    let findings = check(&view, &default_policy());

    assert_eq!(
        findings,
        [Finding {
            check: Check::ProtectedPath,
            mode: Mode::Approval,
            fingerprint: Some("path:.forgejo/workflows/ci.yml".to_owned()),
            path: ".forgejo/workflows/ci.yml".to_owned(),
            line: None,
            detail: "matches .forgejo/workflows/**".to_owned(),
            secret: None,
        }]
    );
    assert_eq!(
        PushCheck::from(Check::ProtectedPath).reason_code(),
        Some("df:protected-path")
    );
    assert_eq!(
        verdict(findings.clone(), &[]),
        Verdict::Reject {
            findings,
            approval_requested: true,
        }
    );
}

#[test]
fn a_changed_repository_configuration_is_always_rejected() {
    let mut policy = default_policy();
    policy.protected_globs.push(".dark-factory/**".to_owned());
    let view = view(vec![commit(2, &[".dark-factory/config.toml"])], vec![]);

    let findings = check(&view, &policy);

    assert_eq!(kinds(&findings), [(Check::ProtectedPath, Mode::Reject)]);
    assert_eq!(
        findings[0].fingerprint.as_deref(),
        Some("path:.dark-factory/config.toml")
    );
    assert_eq!(findings[0].detail, "matches .dark-factory/**");
    assert!(matches!(
        verdict(findings, &[]),
        Verdict::Reject {
            approval_requested: false,
            ..
        }
    ));
}

#[test]
fn deleting_a_protected_path_is_reported() {
    let mut renamed = commit(2, &["ci.yml"]);
    renamed.changes.push(PathChange {
        path: ".github/workflows/ci.yml".to_owned(),
        old: Some(id(50)),
        new: None,
    });
    let view = view(vec![renamed], vec![]);

    let findings = check(&view, &default_policy());

    let paths: Vec<&str> = findings.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, [".github/workflows/ci.yml"]);
}

#[test]
fn a_single_star_does_not_cross_directories() {
    let mut policy = default_policy();
    policy.protected_globs = vec![".forgejo/workflows/*".to_owned(), "deploy/**".to_owned()];
    let view = view(
        vec![commit(
            2,
            &[
                ".forgejo/workflows/sub/ci.yml",
                ".forgejo/workflows/ci.yml",
                "deploy/values.yaml",
            ],
        )],
        vec![],
    );

    let findings = check(&view, &policy);

    let paths: Vec<&str> = findings.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, [".forgejo/workflows/ci.yml", "deploy/values.yaml"]);
}

#[test]
fn a_path_changed_in_two_commits_is_reported_once() {
    let view = view(
        vec![commit(3, &["CODEOWNERS"]), commit(2, &["CODEOWNERS"])],
        vec![],
    );

    let findings = check(&view, &default_policy());

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].fingerprint.as_deref(), Some("path:CODEOWNERS"));
}

#[test]
fn a_blob_over_the_limit_is_too_large() {
    let view = view(
        vec![commit(2, &["data.bin", "small.bin"])],
        vec![
            blob(7, 8 * MIB, &["data.bin"]),
            blob(8, 5 * MIB, &["small.bin"]),
        ],
    );

    let findings = check(&view, &default_policy());

    assert_eq!(
        findings,
        [Finding {
            check: Check::BlobTooLarge,
            mode: Mode::Approval,
            fingerprint: Some(format!("size:data.bin:{}", id(7))),
            path: "data.bin".to_owned(),
            line: None,
            detail: "8388608 bytes, limit 5242880 bytes".to_owned(),
            secret: None,
        }]
    );
    assert_eq!(
        PushCheck::from(Check::BlobTooLarge).reason_code(),
        Some("df:blob-too-large")
    );
}

#[test]
fn a_merge_commit_is_rejected_only_with_linear_history() {
    let mut merge = commit(3, &[]);
    merge.parents.push(id(9));
    let view = view(vec![merge], vec![]);
    let mut linear = default_policy();
    linear.linear_history = true;

    let findings = check(&view, &linear);

    assert_eq!(
        findings,
        [Finding {
            check: Check::MergeCommit,
            mode: Mode::Reject,
            fingerprint: None,
            path: String::new(),
            line: None,
            detail: format!("commit {} has 2 parents", id(3)),
            secret: None,
        }]
    );
    assert_eq!(
        PushCheck::from(Check::MergeCommit).reason_code(),
        Some("df:merge-commit")
    );
    assert_eq!(check(&view, &default_policy()), []);
}

#[test]
fn a_secret_is_reported_with_rule_blob_and_line() {
    let view = view(
        vec![commit(2, &["config.py"])],
        vec![blob(7, 100, &["config.py", "copy.py"])],
    );
    let secrets = FakeSecrets::with(id(7), "betterleaks.github-pat", 3);

    let findings = check_with(&view, &default_policy(), Target::TaskBranch, &secrets);

    assert_eq!(
        findings,
        [Finding {
            check: Check::SecretScan,
            mode: Mode::Approval,
            fingerprint: Some(format!("secret:betterleaks.github-pat:{}:3", id(7))),
            path: "config.py".to_owned(),
            line: Some(3),
            detail: "betterleaks.github-pat ghp_********".to_owned(),
            secret: Some(MaskedSecret {
                rule: "betterleaks.github-pat".to_owned(),
                masked: "ghp_********".to_owned(),
            }),
        }]
    );
    assert_eq!(
        PushCheck::from(Check::SecretScan).reason_code(),
        Some("df:secret-scan")
    );
}

#[test]
fn a_wip_push_only_scans_secrets_and_sizes_and_always_rejects() {
    let mut merge = commit(
        3,
        &[".dark-factory/config.toml", ".github/workflows/ci.yml"],
    );
    merge.author_email = "peter@example.org".to_owned();
    merge.parents.push(id(9));
    let view = view(
        vec![merge],
        vec![blob(7, 100, &["key.pem"]), blob(8, 8 * MIB, &["data.bin"])],
    );
    let mut policy = default_policy();
    policy.linear_history = true;
    let secrets = FakeSecrets::with(id(7), "betterleaks.private-key", 1);

    let findings = check_with(&view, &policy, Target::Wip, &secrets);

    assert_eq!(
        kinds(&findings),
        [
            (Check::BlobTooLarge, Mode::Reject),
            (Check::SecretScan, Mode::Reject),
        ]
    );
    assert!(matches!(
        verdict(findings, &[]),
        Verdict::Reject {
            approval_requested: false,
            ..
        }
    ));
}

#[test]
fn mixed_findings_are_rejected_without_approval() {
    let mut human = commit(2, &[".forgejo/workflows/ci.yml"]);
    human.author_email = "peter@example.org".to_owned();
    let view = view(vec![human], vec![]);

    let findings = check(&view, &default_policy());

    assert_eq!(
        kinds(&findings),
        [
            (Check::Identity, Mode::Reject),
            (Check::ProtectedPath, Mode::Approval),
        ]
    );
    assert!(matches!(
        verdict(findings, &[]),
        Verdict::Reject {
            approval_requested: false,
            ..
        }
    ));
}

#[test]
fn an_unspecified_mode_is_reject() {
    let mut policy = default_policy();
    policy.protected_mode = CheckMode::Unspecified.into();
    let view = view(vec![commit(2, &["CODEOWNERS"])], vec![]);

    assert_eq!(
        kinds(&check(&view, &policy)),
        [(Check::ProtectedPath, Mode::Reject)]
    );
}

#[test]
fn an_invalid_glob_is_an_error() {
    let mut policy = default_policy();
    policy.protected_globs.push("deploy/[".to_owned());

    assert!(CheckPolicy::new(&policy).is_err());
}

#[test]
fn a_failing_secret_scan_fails_the_check() {
    let view = view(vec![commit(2, &["a.bin"])], vec![blob(7, 10, &["a.bin"])]);
    let secrets = FakeSecrets {
        failing: Some(id(7)),
        ..FakeSecrets::default()
    };
    let policy = CheckPolicy::new(&default_policy()).unwrap();

    let error = check_push(
        &view,
        &policy,
        &Identity { email: AGENT },
        Target::TaskBranch,
        &secrets,
    )
    .unwrap_err();

    assert_eq!(error.blob, id(7));
}

#[test]
fn an_exception_passes_the_matching_fingerprint() {
    let findings = vec![approval_finding("path:CODEOWNERS")];

    assert_eq!(
        verdict(findings, &["path:CODEOWNERS".to_owned()]),
        Verdict::Forward
    );
}

#[test]
fn an_exception_does_not_cover_a_different_secret() {
    let exception = format!("secret:betterleaks.github-pat:{}:3", id(7));
    let other_line = format!("secret:betterleaks.github-pat:{}:4", id(7));
    let other_blob = format!("secret:betterleaks.github-pat:{}:3", id(8));
    let findings = vec![
        approval_finding(&exception),
        approval_finding(&other_line),
        approval_finding(&other_blob),
    ];

    let verdict = verdict(findings, &[exception]);

    assert_eq!(
        verdict,
        Verdict::Reject {
            findings: vec![approval_finding(&other_line), approval_finding(&other_blob)],
            approval_requested: true,
        }
    );
}

#[test]
fn exceptions_do_not_pass_reject_findings() {
    let finding = Finding {
        mode: Mode::Reject,
        ..approval_finding("path:.dark-factory/config.toml")
    };

    let verdict = verdict(
        vec![finding.clone()],
        &["path:.dark-factory/config.toml".to_owned()],
    );

    assert_eq!(
        verdict,
        Verdict::Reject {
            findings: vec![finding],
            approval_requested: false,
        }
    );
}

#[test]
fn checks_and_modes_map_to_their_contract_values() {
    use df_proto::gateway::v1::PushCheck;

    let checks = [
        (Check::SecretScan, PushCheck::SecretScan),
        (Check::Identity, PushCheck::Identity),
        (Check::ProtectedPath, PushCheck::ProtectedPath),
        (Check::BlobTooLarge, PushCheck::BlobTooLarge),
        (Check::MergeCommit, PushCheck::MergeCommit),
    ];
    for (check, expected) in checks {
        assert_eq!(PushCheck::from(check), expected);
    }
    assert_eq!(CheckMode::from(Mode::Reject), CheckMode::Reject);
    assert_eq!(CheckMode::from(Mode::Approval), CheckMode::Approval);
}
