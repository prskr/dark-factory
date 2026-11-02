//! Pushes after a turn that the Git proxy rejects with a `df:` reason (iter2-push-checks design
//! D6, `specs/workspace-lifecycle/spec.md`, "Push rejected by a check"). The fake remote's
//! `proc-receive` hook rejects every ref of the next push with a given reason once, like the
//! proxy's `ng <ref> <reason>`, and lets later pushes through.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use df_proto::agent::v1::{PushStatus, PushTarget, Workspace};

use super::{RunScenario, clone_for_workspace_test, setup_test_remote_repo};
use crate::git::Git;
use crate::workspace::{push_after_turn, rejected_push};

const TASK_BRANCH: &str = "factory/task/DF-1";

/// The `proc-receive` protocol (githooks(5)): version negotiation, the commands, then one result
/// per ref. `@REASON@` is the file with the reason of the next rejection.
const PROC_RECEIVE_HOOK: &str = r#"#!/bin/sh
LC_ALL=C; export LC_ALL
reason_file='@REASON@'
pkt() { printf '%04x%s' $(( ${#1} + 4 )) "$1"; }
read_len() { len=$(dd bs=1 count=4 2>/dev/null); [ -n "$len" ] && [ "$len" != 0000 ]; }
while read_len; do dd bs=1 count=$(( 0x$len - 4 )) >/dev/null 2>&1; done
pkt 'version=1'; printf 0000
refs=
while read_len; do
  command=$(dd bs=1 count=$(( 0x$len - 4 )) 2>/dev/null)
  refs="$refs ${command##* }"
done
for ref in $refs; do
  if [ -f "$reason_file" ]; then pkt "ng $ref $(cat "$reason_file")"
  else pkt "ok $ref"; pkt 'option fall-through'; fi
done
rm -f "$reason_file"
printf 0000
"#;

/// An upstream with `main` and the rejecting hook, and a local clone on the task branch.
struct RejectingRemote {
    _temp: tempfile::TempDir,
    upstream: PathBuf,
    local: PathBuf,
    git: Git,
    workspace: Workspace,
}

impl RejectingRemote {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let upstream = setup_test_remote_repo(temp.path());
        install_rejecting_hook(&upstream);
        let local = temp.path().join("local");
        clone_for_workspace_test(&upstream, &local);
        run_git(&local, &["checkout", "-b", TASK_BRANCH]);
        let hooks = temp.path().join("hooks-empty");
        std::fs::create_dir_all(&hooks).unwrap();
        let git = Git::new(&local)
            .allow_protocols(["file"])
            .with_hooks_empty_dir(&hooks)
            .with_retry_delays(Vec::new());
        let workspace = Workspace {
            repo_url: format!("file://{}", upstream.display()),
            base_branch: "main".into(),
            task_branch: TASK_BRANCH.into(),
            wip_branch: "factory/wip/DF-1".into(),
            commit_identity: None,
            git_hooks: vec![],
            git_config: vec![],
        };
        Self {
            _temp: temp,
            upstream,
            local,
            git,
            workspace,
        }
    }

    /// The next push is rejected with `reason`.
    fn reject_next(&self, reason: &str) {
        reject_next_push(&self.upstream, reason);
    }

    /// A turn's commit; returns its SHA.
    fn commit(&self, file: &str) -> String {
        std::fs::write(self.local.join(file), file).unwrap();
        run_git(&self.local, &["add", file]);
        run_git(&self.local, &["commit", "-m", file]);
        run_git(&self.local, &["rev-parse", "HEAD"])
    }

    /// SHA of the task branch on the remote; empty if it does not exist.
    fn remote_task_branch(&self) -> String {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", TASK_BRANCH])
            .current_dir(&self.upstream)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
}

/// Installs the `proc-receive` hook in the bare repository `upstream`.
fn install_rejecting_hook(upstream: &Path) {
    let hook = upstream.join("hooks/proc-receive");
    let reason = upstream.join("df-reason");
    std::fs::write(
        &hook,
        PROC_RECEIVE_HOOK.replace("@REASON@", reason.to_str().unwrap()),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    run_git(
        upstream,
        &["config", "receive.procReceiveRefs", "refs/heads"],
    );
}

/// The next push to `upstream` is rejected with `reason`.
fn reject_next_push(upstream: &Path, reason: &str) {
    std::fs::write(upstream.join("df-reason"), reason).unwrap();
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A failed `git push --porcelain` whose ref was rejected with `reason`.
fn rejected(reason: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(256),
        stdout: format!(
            "To https://gateway/factory/demo.git\n!\tHEAD:refs/heads/{TASK_BRANCH}\t[remote rejected] ({reason})\nDone\n"
        )
        .into_bytes(),
        stderr: b"error: failed to push some refs to 'https://gateway/factory/demo.git'\n".to_vec(),
    }
}

#[test]
fn df_reasons_map_to_push_statuses_and_a_secret_among_findings_wins() {
    let cases = [
        (
            "df:secret-scan .env:1 (betterleaks.github-pat ghp_********)",
            PushStatus::RejectedSecretDetected,
        ),
        ("df:identity", PushStatus::RejectedByCheck),
        (
            "df:protected-path CODEOWNERS (matches CODEOWNERS)",
            PushStatus::RejectedByCheck,
        ),
        (
            "df:blob-too-large data.bin (7340032 bytes)",
            PushStatus::RejectedByCheck,
        ),
        ("df:merge-commit", PushStatus::RejectedByCheck),
        ("df:push-too-large", PushStatus::RejectedByCheck),
        ("df:non-fast-forward", PushStatus::RejectedNonFastForward),
        ("df:audit-unavailable", PushStatus::Failed),
        ("df:ref-not-allowed", PushStatus::Failed),
        ("df:check-failed", PushStatus::Failed),
        ("df:other-ref-rejected", PushStatus::Failed),
        ("df:a-reason-from-a-newer-gateway", PushStatus::Failed),
        // A secret among other findings wins.
        (
            "df:protected-path CODEOWNERS (matches CODEOWNERS); df:secret-scan a.py:3 (rule ghp_****)",
            PushStatus::RejectedSecretDetected,
        ),
    ];
    for (reason, status) in cases {
        assert_eq!(
            rejected_push(&rejected(reason)),
            (status, reason.to_owned()),
            "{reason}"
        );
    }
}

#[test]
fn a_rejection_without_a_df_reason_keeps_the_whole_output_as_detail() {
    let (status, detail) = rejected_push(&rejected("non-fast-forward"));
    assert_eq!(status, PushStatus::RejectedNonFastForward);
    assert!(
        detail.contains("[remote rejected] (non-fast-forward)"),
        "{detail}"
    );

    let unreachable = Output {
        status: ExitStatus::from_raw(32768),
        stdout: Vec::new(),
        stderr: b"fatal: unable to access 'https://gateway/': Could not resolve host\n".to_vec(),
    };
    let (status, detail) = rejected_push(&unreachable);
    assert_eq!(status, PushStatus::Failed);
    assert!(detail.contains("Could not resolve host"), "{detail}");
}

#[tokio::test]
async fn a_check_rejection_is_reported_and_the_branch_is_pushed_again_after_the_next_turn() {
    let remote = RejectingRemote::new();
    let reason = "df:protected-path .forgejo/workflows/ci.yml (matches .forgejo/workflows/**)";
    remote.reject_next(reason);
    let head = remote.commit("ci.yml");
    let mut last_pushed = None;

    let report = push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
        .await
        .unwrap()
        .expect("a push after the turn");

    assert_eq!(report.status(), PushStatus::RejectedByCheck);
    assert_eq!(report.detail, reason);
    assert_eq!(report.target(), PushTarget::TaskBranch);
    assert_eq!(report.sha, head);
    assert_eq!(last_pushed, None, "rejected commits count as not pushed");
    assert_eq!(remote.remote_task_branch(), "");

    // The next turn adds no commit: the same commits are pushed again.
    let report = push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
        .await
        .unwrap()
        .expect("a push after the next turn");

    assert_eq!(report.status(), PushStatus::Ok);
    assert_eq!(last_pushed.as_deref(), Some(head.as_str()));
    assert_eq!(remote.remote_task_branch(), head);
}

#[tokio::test]
async fn an_unavailable_audit_fails_the_push_and_it_is_pushed_again_after_the_next_turn() {
    let remote = RejectingRemote::new();
    let first = remote.commit("a.txt");
    let mut last_pushed = None;
    push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
        .await
        .unwrap();
    assert_eq!(last_pushed.as_deref(), Some(first.as_str()));
    remote.reject_next("df:audit-unavailable");
    let second = remote.commit("b.txt");

    let report = push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
        .await
        .unwrap()
        .expect("a push after the turn");

    assert_eq!(report.status(), PushStatus::Failed);
    assert_eq!(report.detail, "df:audit-unavailable");
    assert_eq!(last_pushed.as_deref(), Some(first.as_str()));
    assert_eq!(remote.remote_task_branch(), first);

    let report = push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
        .await
        .unwrap()
        .expect("a push after the next turn");
    assert_eq!(report.status(), PushStatus::Ok);
    assert_eq!(remote.remote_task_branch(), second);
}

#[tokio::test]
async fn a_check_rejection_of_the_prepare_push_counts_nothing_as_pushed_and_the_turn_pushes_again()
{
    use df_proto::agent::v1::agent_message;

    let script = r#"
sessions:
  new:
    - - say: "nothing to commit"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-prepare-rejected", script) else {
        return;
    };
    // A killed earlier run left an unpushed commit on the task branch.
    let repo = scenario.temp.path().join("repo");
    clone_for_workspace_test(&scenario.upstream, &repo);
    run_git(&repo, &["checkout", "-q", "-b", TASK_BRANCH]);
    std::fs::create_dir_all(repo.join(".forgejo/workflows")).unwrap();
    std::fs::write(repo.join(".forgejo/workflows/ci.yml"), "on: push\n").unwrap();
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-q", "-m", "ci"]);
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    install_rejecting_hook(&scenario.upstream);
    let reason = "df:protected-path .forgejo/workflows/ci.yml (matches .forgejo/workflows/**)";
    reject_next_push(&scenario.upstream, reason);

    let (_, messages) = scenario.release_up_front().run(|_, _| {}).await;

    let reports: Vec<(PushTarget, PushStatus, String, String)> = messages
        .into_iter()
        .filter_map(|message| match message.body {
            Some(agent_message::Body::PushReport(report)) => Some((
                report.target(),
                report.status(),
                report.sha.clone(),
                report.detail,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        reports,
        [
            (
                PushTarget::TaskBranch,
                PushStatus::RejectedByCheck,
                head.clone(),
                reason.to_owned()
            ),
            (
                PushTarget::TaskBranch,
                PushStatus::Ok,
                head.clone(),
                String::new()
            ),
        ]
    );
}

#[tokio::test]
async fn a_check_rejection_of_the_prepare_push_fails_the_prepare_push() {
    use crate::workspace::prepare_workspace;

    let remote = RejectingRemote::new();
    let head = remote.commit("ci.yml");
    let reason = "df:protected-path ci.yml (matches ci.yml)";
    remote.reject_next(reason);
    let hooks = remote.local.with_file_name("hooks-empty");

    let prepared = prepare_workspace(&remote.git, &remote.local, &remote.workspace, &hooks, None)
        .await
        .unwrap();

    assert!(prepared.push_failed);
    assert_eq!(prepared.head_sha, head);
    let report = prepared.push_report.expect("a prepare push");
    assert_eq!(report.status(), PushStatus::RejectedByCheck);
    assert_eq!(report.detail, reason);
    assert_eq!(remote.remote_task_branch(), "");
}

#[tokio::test]
async fn a_rejected_wip_snapshot_is_reported_with_its_reason() {
    use df_proto::agent::v1::RunOutcome;

    use crate::workspace::{FinalizeParams, finalize_workspace};

    let remote = RejectingRemote::new();
    let mut last_pushed = Some(run_git(&remote.local, &["rev-parse", "HEAD"]));
    std::fs::write(remote.local.join("notes.txt"), "uncommitted").unwrap();
    let reason = "df:secret-scan notes.txt:1 (betterleaks.github-pat ghp_********)";
    remote.reject_next(reason);
    let runtime = remote.local.with_file_name("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    let hooks = remote.local.with_file_name("hooks-empty");
    let params = FinalizeParams {
        repo_dir: &remote.local,
        runtime_dir: &runtime,
        workspace: &remote.workspace,
        run_id: "run-1",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 1024 * 1024,
        diverged: false,
    };

    let result = finalize_workspace(&remote.git, &params, &mut last_pushed)
        .await
        .unwrap();

    let [wip] = &result.push_reports[..] else {
        panic!("{:?}", result.push_reports);
    };
    assert_eq!(wip.target(), PushTarget::WipBranch);
    assert_eq!(wip.status(), PushStatus::RejectedSecretDetected);
    assert_eq!(wip.detail, reason);
    assert!(result.run_report.workspace.unwrap().wip_sha.is_empty());
}

#[tokio::test]
async fn a_ref_rule_rejection_is_a_non_fast_forward_and_never_forced() {
    let mut remote = RejectingRemote::new();
    // Records the arguments of every git invocation of the runtime.
    let log = remote.local.with_file_name("git-args.log");
    let wrapper = remote.local.with_file_name("git-wrapper");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec git \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    remote.git = remote.git.clone().with_program(&wrapper);
    let mut last_pushed = None;
    for turn in ["a.txt", "b.txt"] {
        remote.reject_next("df:non-fast-forward");
        remote.commit(turn);

        let report = push_after_turn(&remote.git, &remote.workspace, &mut last_pushed)
            .await
            .unwrap()
            .expect("a push after the turn");

        assert_eq!(
            report.status(),
            PushStatus::RejectedNonFastForward,
            "{turn}"
        );
        assert_eq!(report.detail, "df:non-fast-forward", "{turn}");
        assert_eq!(last_pushed, None, "{turn}");
    }
    let pushes: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter(|args| args.contains(" push "))
        .map(str::to_owned)
        .collect();
    assert_eq!(pushes.len(), 2, "{pushes:?}");
    for push in &pushes {
        assert!(!push.contains("--force") && !push.contains(" +"), "{push}");
    }
}
