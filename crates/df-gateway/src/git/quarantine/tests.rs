use std::path::Path;
use std::process::Command;

use super::*;

const LIMIT: usize = 64 * 1024 * 1024;
static NOT_INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Mirror with A→B on `factory/task/DF-7`; a client adds C on B and D off A.
struct Fixture {
    root: TempDir,
    b: ObjectId,
    c: ObjectId,
    d: ObjectId,
}

impl Fixture {
    fn mirror(&self) -> PathBuf {
        self.root.path().join("mirror.git")
    }

    fn work(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.path().join("quarantine")
    }

    /// A thin pack of `tips` excluding everything reachable from B.
    fn thin_pack(&self, tips: &[&ObjectId]) -> Vec<u8> {
        let mut revs: Vec<String> = tips.iter().map(ToString::to_string).collect();
        revs.push(format!("^{}", self.b));
        let input = revs.join("\n") + "\n";
        let mut child = Command::new("git")
            .args(["pack-objects", "--thin", "--stdout", "--revs", "-q"])
            .current_dir(self.work())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        out.stdout
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
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

fn commit(work: &Path, line: usize, text: &str, msg: &str) -> ObjectId {
    let content: String = (1..=400)
        .map(|i| {
            if i == line {
                format!("{i} {text}\n")
            } else {
                format!("{i} some line content to make deltas worthwhile\n")
            }
        })
        .collect();
    std::fs::write(work.join("file.txt"), content).unwrap();
    git(work, &["add", "file.txt"]);
    git(work, &["commit", "-q", "-m", msg]);
    ObjectId::parse(&git(work, &["rev-parse", "HEAD"])).unwrap()
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let mirror = root.path().join("mirror.git");
    let work = root.path().join("work");
    git(
        root.path(),
        &["init", "-q", "--bare", mirror.to_str().unwrap()],
    );
    git(
        root.path(),
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    let a = commit(&work, 0, "", "A");
    let b = commit(&work, 50, "CHANGED-B", "B");
    git(
        &work,
        &[
            "push",
            "-q",
            mirror.to_str().unwrap(),
            "HEAD:refs/heads/factory/task/DF-7",
        ],
    );
    let c = commit(&work, 300, "CHANGED-C", "C");
    git(&work, &["checkout", "-q", "-b", "other", a.as_str()]);
    let d = commit(&work, 200, "CHANGED-D", "D");
    Fixture { root, b, c, d }
}

fn mirror_pack_files(f: &Fixture) -> usize {
    std::fs::read_dir(f.mirror().join("objects/pack"))
        .unwrap()
        .count()
}

#[test]
fn thin_pack_is_indexed_against_the_mirror() {
    let f = fixture();
    let pack = f.thin_pack(&[&f.c, &f.d]);

    let q = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &NOT_INTERRUPTED,
    )
    .unwrap();

    let extensions: Vec<String> = std::fs::read_dir(q.objects_dir().join("pack"))
        .unwrap()
        .filter_map(|e| {
            let path = e.unwrap().path();
            path.extension().map(|x| x.to_string_lossy().into_owned())
        })
        .collect();
    assert!(extensions.iter().any(|x| x == "pack"), "{extensions:?}");
    assert!(extensions.iter().any(|x| x == "idx"), "{extensions:?}");
    let alternates = std::fs::read_to_string(q.objects_dir().join("info/alternates")).unwrap();
    assert!(alternates.trim_end().ends_with("objects"));
}

#[test]
fn ancestry_spans_mirror_and_quarantine() {
    let f = fixture();
    let pack = f.thin_pack(&[&f.c, &f.d]);
    let q = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &NOT_INTERRUPTED,
    )
    .unwrap();

    assert!(q.is_ancestor(&f.b, &f.c).unwrap());
    assert!(!q.is_ancestor(&f.b, &f.d).unwrap());
    assert!(q.is_ancestor(&f.c, &f.c).unwrap());
}

#[test]
fn quarantine_leaves_the_mirror_unchanged() {
    let f = fixture();
    let before = mirror_pack_files(&f);
    let pack = f.thin_pack(&[&f.c]);

    let q = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &NOT_INTERRUPTED,
    )
    .unwrap();

    assert_eq!(mirror_pack_files(&f), before);
    let mirror = gix::open(f.mirror()).unwrap();
    let c = to_gix(&f.c).unwrap();
    assert!(mirror.find_object(c).is_err());
    drop(q);
}

#[test]
fn dropping_the_quarantine_removes_its_directory() {
    let f = fixture();
    let pack = f.thin_pack(&[&f.c]);
    let q = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &NOT_INTERRUPTED,
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(f.quarantine_dir()).unwrap().count(), 1);

    drop(q);

    assert_eq!(std::fs::read_dir(f.quarantine_dir()).unwrap().count(), 0);
}

#[test]
fn corrupt_pack_is_an_error() {
    let f = fixture();
    let mut pack = f.thin_pack(&[&f.c]);
    let middle = pack.len() / 2;
    pack[middle] ^= 0xff;

    let err = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &NOT_INTERRUPTED,
    )
    .unwrap_err();

    assert!(matches!(err, QuarantineError::Pack(_)), "{err}");
    assert_eq!(std::fs::read_dir(f.quarantine_dir()).unwrap().count(), 0);
}

#[test]
fn an_interrupted_indexing_is_an_error() {
    let f = fixture();
    let pack = f.thin_pack(&[&f.c, &f.d]);
    let interrupt = AtomicBool::new(true);

    let err = quarantine(
        &f.mirror(),
        pack.as_slice(),
        &f.quarantine_dir(),
        LIMIT,
        &interrupt,
    )
    .unwrap_err();

    assert!(matches!(err, QuarantineError::Pack(_)), "{err}");
}
