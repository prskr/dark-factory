use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gix::bstr::ByteSlice;
use tempfile::TempDir;

use super::*;
use crate::git::quarantine::quarantine;

const LIMIT: usize = 64 * 1024 * 1024;
static NOT_INTERRUPTED: AtomicBool = AtomicBool::new(false);
const AGENT: &str = "agent@factory.test";
const HUMAN: &str = "peter@example.org";
/// The branches of the pushing run, task `DF-7`.
const TASK: TaskRefs<'static> = TaskRefs {
    task_branch: "refs/heads/factory/task/DF-7",
    wip_branch: "refs/heads/factory/wip/DF-7",
};

/// A bare mirror (what the forge has) and a working repository whose commits are pushed.
struct Repos {
    root: TempDir,
}

impl Repos {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), AGENT, &["init", "-q", "--bare", "mirror.git"]);
        git(root.path(), AGENT, &["init", "-q", "-b", "main", "work"]);
        Self { root }
    }

    fn mirror(&self) -> PathBuf {
        self.root.path().join("mirror.git")
    }

    fn work(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.work(), AGENT, args)
    }

    fn write(&self, path: &str, content: &[u8]) {
        let path = self.work().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn rev(&self, rev: &str) -> ObjectId {
        ObjectId::parse(&self.git(&["rev-parse", rev])).unwrap()
    }

    /// Commits every change of the working tree as `email` (author and committer).
    fn commit_as(&self, email: &str, message: &str) -> ObjectId {
        git(&self.work(), email, &["add", "-A"]);
        git(
            &self.work(),
            email,
            &["commit", "-q", "--allow-empty", "-m", message],
        );
        self.rev("HEAD")
    }

    fn commit(&self, message: &str) -> ObjectId {
        self.commit_as(AGENT, message)
    }

    /// Pushes `refspec` to the mirror, as if the forge already had it.
    fn publish(&self, refspec: &str) {
        let mirror = self.mirror();
        self.git(&["push", "-q", mirror.to_str().unwrap(), refspec]);
    }

    /// The view of a push of `tip`: a thin pack without what the mirror has, quarantined.
    fn view(&self, tip: &ObjectId, max_commits: usize) -> Result<PushView, ViewError> {
        self.view_with(tip, max_commits, &NOT_INTERRUPTED)
    }

    fn view_with(
        &self,
        tip: &ObjectId,
        max_commits: usize,
        interrupt: &AtomicBool,
    ) -> Result<PushView, ViewError> {
        let mirror_tips = git(
            &self.mirror(),
            AGENT,
            &["for-each-ref", "--format=%(objectname)"],
        );
        let mut revs = vec![tip.to_string()];
        revs.extend(mirror_tips.lines().map(|tip| format!("^{tip}")));
        let pack = pack_objects(&self.work(), &revs);
        let quarantined = quarantine(
            &self.mirror(),
            pack.as_slice(),
            &self.root.path().join("quarantine"),
            LIMIT,
            &NOT_INTERRUPTED,
        )
        .unwrap();
        quarantined.push_view(TASK, tip, max_commits, interrupt)
    }
}

fn git(dir: &Path, email: &str, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn pack_objects(work: &Path, revs: &[String]) -> Vec<u8> {
    let mut child = Command::new("git")
        .args(["pack-objects", "--thin", "--stdout", "--revs", "-q"])
        .current_dir(work)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = revs.join("\n") + "\n";
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    out.stdout
}

fn oid(id: &ObjectId) -> gix::ObjectId {
    gix::ObjectId::from_hex(id.as_str().as_bytes()).unwrap()
}

fn commit_ids(view: &PushView) -> Vec<gix::ObjectId> {
    view.commits.iter().map(|commit| commit.id).collect()
}

fn changed_paths(view: &PushView) -> Vec<&str> {
    let mut paths: Vec<&str> = view
        .commits
        .iter()
        .flat_map(|commit| &commit.changes)
        .map(|change| change.path.as_str())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

#[test]
fn a_rebased_branch_adds_only_the_agents_commits() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "-b", "task"]);
    r.write("agent.txt", b"agent work\n");
    r.commit("agent 1");
    r.publish("HEAD:refs/heads/factory/task/DF-7");
    r.git(&["checkout", "-q", "main"]);
    r.write("human.txt", b"human work\n");
    r.commit_as(HUMAN, "human 1");
    r.write(".forgejo/workflows/ci.yml", b"on: push\n");
    r.commit_as(HUMAN, "human 2");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "task"]);
    r.git(&["rebase", "-q", "main"]);
    let rebased = r.rev("HEAD");
    r.write("agent.txt", b"more agent work\n");
    let tip = r.commit("agent 2");

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&tip), oid(&rebased)]);
    assert_eq!(changed_paths(&view), ["agent.txt"]);
    let mut blobs: Vec<(gix::ObjectId, u64)> =
        view.blobs.iter().map(|blob| (blob.id, blob.size)).collect();
    blobs.sort_unstable_by_key(|(_, size)| *size);
    assert_eq!(
        blobs,
        [
            (oid(&r.rev("HEAD~:agent.txt")), 11),
            (oid(&r.rev("HEAD:agent.txt")), 16),
        ],
        "both versions of `agent.txt`, none of the human's blobs"
    );
    assert!(
        view.commits
            .iter()
            .all(|commit| commit.author_email == AGENT)
    );
}

#[test]
fn a_rename_is_a_deletion_plus_an_addition() {
    let r = Repos::new();
    r.write("docs/old.md", b"text\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.git(&["mv", "docs/old.md", "docs/new.md"]);
    let tip = r.commit("rename");
    let blob = oid(&r.rev("HEAD:docs/new.md"));

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&tip)]);
    assert_eq!(
        view.commits[0].changes,
        [
            PathChange {
                path: "docs/new.md".to_owned(),
                old: None,
                new: Some(blob),
            },
            PathChange {
                path: "docs/old.md".to_owned(),
                old: Some(blob),
                new: None,
            },
        ]
    );
    assert_eq!(
        view.blobs,
        [AddedBlob {
            id: blob,
            size: 5,
            paths: vec!["docs/new.md".to_owned()],
        }],
        "the added path is checked, also with a blob the mirror has"
    );
}

#[test]
fn a_root_commit_is_diffed_against_the_empty_tree() {
    let r = Repos::new();
    r.write("a.txt", b"hello\n");
    r.write("dir/b.bin", &[0, 1, 2, 3, 255]);
    let root = r.commit("root");

    let view = r.view(&root, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&root)]);
    let commit = &view.commits[0];
    assert!(commit.parents.is_empty());
    assert_eq!(commit.author_email, AGENT);
    assert_eq!(commit.committer_email, AGENT);
    assert_eq!(changed_paths(&view), ["a.txt", "dir/b.bin"]);
    assert!(commit.changes.iter().all(|change| change.old.is_none()));
    let mut sizes: Vec<(&str, u64)> = view
        .blobs
        .iter()
        .map(|blob| (blob.paths[0].as_str(), blob.size))
        .collect();
    sizes.sort_unstable();
    assert_eq!(sizes, [("a.txt", 6), ("dir/b.bin", 5)]);
}

#[test]
fn a_merge_reports_only_the_paths_that_differ_from_every_parent() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "-b", "task"]);
    r.write("agent.txt", b"agent work\n");
    let task = r.commit("agent");
    r.publish("HEAD:refs/heads/factory/task/DF-7");
    r.git(&["checkout", "-q", "main"]);
    r.write("human.txt", b"human work\n");
    let main = r.commit_as(HUMAN, "human");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "task"]);
    r.git(&["merge", "-q", "--no-edit", "main"]);
    let merge = r.rev("HEAD");

    let view = r.view(&merge, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&merge)]);
    assert_eq!(view.commits[0].parents, [oid(&task), oid(&main)]);
    assert!(view.commits[0].changes.is_empty(), "{view:?}");
    assert!(view.blobs.is_empty());
}

#[test]
fn a_merge_reports_a_path_it_changes_itself() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "-b", "task"]);
    r.write("readme.md", b"agent\n");
    r.commit("agent");
    r.publish("HEAD:refs/heads/factory/task/DF-7");
    r.git(&["checkout", "-q", "main"]);
    r.write("readme.md", b"human\n");
    r.commit_as(HUMAN, "human");
    r.publish("HEAD:refs/heads/main");
    r.git(&["checkout", "-q", "task"]);
    r.git(&["merge", "-q", "--no-commit", "-s", "ours", "main"]);
    r.write("readme.md", b"resolved\n");
    r.write(".dark-factory/config.toml", b"evil = true\n");
    let merge = r.commit("merge");

    let view = r.view(&merge, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&merge)]);
    assert_eq!(
        changed_paths(&view),
        [".dark-factory/config.toml", "readme.md"]
    );
}

#[test]
fn commits_of_wip_branches_and_other_tasks_are_not_upstream() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.write("wip.txt", b"wip\n");
    let wip = r.commit("wip");
    r.publish("HEAD:refs/heads/factory/wip/DF-7");
    r.write("other.txt", b"other task\n");
    let other = r.commit("other task");
    r.publish("HEAD:refs/heads/factory/task/DF-8");
    r.publish("HEAD:refs/heads/factory/wip/DF-8");
    r.write("agent.txt", b"agent\n");
    let tip = r.commit("agent");

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&tip), oid(&other), oid(&wip)]);
}

#[test]
fn blobs_the_mirror_has_from_wip_branches_and_other_tasks_are_added() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.write("wip.txt", b"wip\n");
    r.commit("wip");
    r.publish("HEAD:refs/heads/factory/wip/DF-7");
    r.write("other.txt", b"other task\n");
    r.commit("other task");
    r.publish("HEAD:refs/heads/factory/task/DF-8");
    r.write("agent.txt", b"agent\n");
    let tip = r.commit("agent");

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    let mut paths: Vec<&str> = view
        .blobs
        .iter()
        .flat_map(|blob| blob.paths.iter().map(String::as_str))
        .collect();
    paths.sort_unstable();
    assert_eq!(paths, ["agent.txt", "other.txt", "wip.txt"]);
}

#[test]
fn commits_of_the_own_task_branch_and_of_human_branches_are_upstream() {
    let r = Repos::new();
    r.write("readme.md", b"base\n");
    r.commit("base");
    r.publish("HEAD:refs/heads/main");
    r.write(".forgejo/workflows/ci.yml", b"on: push\n");
    r.commit_as(HUMAN, "human");
    r.publish("HEAD:refs/heads/peter/ci");
    r.write(".dark-factory/config.toml", b"approved = true\n");
    r.commit("agent 1");
    r.publish("HEAD:refs/heads/factory/task/DF-7");
    r.write("agent.txt", b"agent\n");
    let tip = r.commit("agent 2");

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    assert_eq!(commit_ids(&view), [oid(&tip)]);
    assert_eq!(changed_paths(&view), ["agent.txt"]);
}

#[test]
fn a_blob_added_at_two_paths_is_listed_once() {
    let r = Repos::new();
    r.write("a.txt", b"same\n");
    r.write("b/c.txt", b"same\n");
    let tip = r.commit("copies");

    let view = r.view(&tip, MAX_COMMITS).unwrap();

    assert_eq!(view.blobs.len(), 1);
    assert_eq!(view.blobs[0].paths, ["a.txt", "b/c.txt"]);
}

#[test]
fn more_commits_than_the_bound_are_too_many() {
    let r = Repos::new();
    for message in ["1", "2", "3"] {
        r.commit(message);
    }
    let tip = r.commit("4");

    let error = r.view(&tip, 3).unwrap_err();

    assert!(matches!(error, ViewError::TooManyCommits(3)), "{error}");
    assert_eq!(r.view(&tip, 4).unwrap().commits.len(), 4);
}

#[test]
fn a_tip_the_mirror_already_has_adds_nothing() {
    let r = Repos::new();
    r.write("a.txt", b"hello\n");
    let tip = r.commit("base");
    r.publish("HEAD:refs/heads/main");
    let mirror = gix::open_opts(r.mirror(), gix::open::Options::isolated()).unwrap();

    let view = push_view(&mirror, &mirror, TASK, &tip, MAX_COMMITS, &NOT_INTERRUPTED).unwrap();

    assert_eq!(view, PushView::default());
}

#[test]
fn an_interrupted_view_is_an_error() {
    let r = Repos::new();
    let tip = r.commit("base");

    let error = r
        .view_with(&tip, MAX_COMMITS, &AtomicBool::new(true))
        .unwrap_err();

    assert!(matches!(error, ViewError::Interrupted), "{error}");
}

#[test]
fn paths_escape_backslashes_control_characters_and_invalid_utf8() {
    assert_eq!(escape(b"dir/caf\xc3\xa9.txt".as_bstr()), "dir/café.txt");
    assert_eq!(
        escape(b"a\\b\nc\xffd\x7f".as_bstr()),
        "a\\\\b\\x0ac\\xffd\\x7f"
    );
}
