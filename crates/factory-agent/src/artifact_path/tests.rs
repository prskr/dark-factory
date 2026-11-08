use std::os::unix::fs::symlink;

use super::*;

struct Roots {
    workspace: tempfile::TempDir,
    tmp: tempfile::TempDir,
}

impl Roots {
    fn new() -> Self {
        Self {
            workspace: tempfile::tempdir().unwrap(),
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn roots(&self) -> Vec<PathBuf> {
        vec![
            self.workspace.path().to_path_buf(),
            self.tmp.path().to_path_buf(),
        ]
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        read_artifact_file(&self.roots(), path.to_str().unwrap())
    }
}

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const PDF: &[u8] = b"%PDF-1.7\n\x00\x01";

#[test]
fn artifact_path_reads_a_regular_file_under_either_root() {
    let roots = Roots::new();
    std::fs::write(roots.tmp.path().join("login.png"), PNG_SIGNATURE).unwrap();
    std::fs::create_dir(roots.workspace.path().join("docs")).unwrap();
    std::fs::write(roots.workspace.path().join("docs/a.md"), "# a").unwrap();

    assert_eq!(
        roots.read(&roots.tmp.path().join("login.png")).unwrap(),
        PNG_SIGNATURE
    );
    assert_eq!(
        roots
            .read(&roots.workspace.path().join("docs/a.md"))
            .unwrap(),
        b"# a"
    );
}

#[test]
fn artifact_path_refuses_a_symlinked_directory_and_a_symlinked_file() {
    let roots = Roots::new();
    let secret_dir = tempfile::tempdir().unwrap();
    std::fs::write(secret_dir.path().join("client.key"), "secret").unwrap();
    symlink(secret_dir.path(), roots.workspace.path().join("certs")).unwrap();
    symlink(
        secret_dir.path().join("client.key"),
        roots.workspace.path().join("key.md"),
    )
    .unwrap();

    for path in ["certs/client.key", "key.md"] {
        let err = roots.read(&roots.workspace.path().join(path)).unwrap_err();
        assert!(err.contains("symlink"), "{path}: {err}");
    }
}

#[test]
fn artifact_path_refuses_dotdot_escapes() {
    let roots = Roots::new();
    std::fs::write(roots.tmp.path().join("other.md"), "x").unwrap();
    let escape = roots
        .workspace
        .path()
        .join("../")
        .join(roots.tmp.path().file_name().unwrap());
    let err = roots.read(&escape.join("other.md")).unwrap_err();
    assert!(err.contains("symlink or leaves"), "{err}");
}

#[test]
fn artifact_path_refuses_paths_outside_the_roots_naming_them() {
    let roots = Roots::new();
    let err = roots.read(Path::new("/etc/passwd")).unwrap_err();
    assert!(
        err.contains(&roots.workspace.path().display().to_string()),
        "{err}"
    );
    assert!(
        err.contains(&roots.tmp.path().display().to_string()),
        "{err}"
    );
    assert!(
        roots
            .read(Path::new("relative.md"))
            .unwrap_err()
            .contains("absolute")
    );
}

#[test]
fn artifact_path_refuses_directories_and_missing_files() {
    let roots = Roots::new();
    std::fs::create_dir(roots.workspace.path().join("docs")).unwrap();
    let err = roots
        .read(&roots.workspace.path().join("docs"))
        .unwrap_err();
    assert!(err.contains("not a regular file"), "{err}");
    let err = roots.read(roots.workspace.path()).unwrap_err();
    assert!(err.contains("not a regular file"), "{err}");
    let err = roots
        .read(&roots.workspace.path().join("nope.md"))
        .unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn artifact_path_rejects_a_file_over_8_mib_before_reading() {
    let roots = Roots::new();
    let file = std::fs::File::create(roots.tmp.path().join("big.md")).unwrap();
    // A sparse file: the size is checked from the descriptor, nothing is read.
    file.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
    let err = roots.read(&roots.tmp.path().join("big.md")).unwrap_err();
    assert!(err.contains("over the limit"), "{err}");
}

#[tokio::test]
async fn write_artifact_sends_the_file_as_attachment_without_path() {
    let roots = Roots::new();
    let png = roots.tmp.path().join("login.png");
    std::fs::write(&png, PNG_SIGNATURE).unwrap();
    let args = serde_json::json!({ "name": "login.png", "path": png }).to_string();

    let prepared = prepare_write_artifact(&roots.roots(), &args).await.unwrap();

    assert_eq!(prepared.attachment.as_deref(), Some(PNG_SIGNATURE));
    let sent: Value = serde_json::from_str(&prepared.arguments_json).unwrap();
    assert_eq!(sent, serde_json::json!({ "name": "login.png" }));
}

#[tokio::test]
async fn write_artifact_fails_locally_for_a_pdf_naming_the_allowed_types() {
    let roots = Roots::new();
    let pdf = roots.tmp.path().join("doc.pdf");
    std::fs::write(&pdf, PDF).unwrap();
    let args = serde_json::json!({ "name": "doc.pdf", "path": pdf }).to_string();

    let err = prepare_write_artifact(&roots.roots(), &args)
        .await
        .unwrap_err();

    assert!(err.contains("PNG, JPEG, WebP or GIF"), "{err}");
}

#[tokio::test]
async fn write_artifact_with_content_is_forwarded_unchanged() {
    let args = r##"{"name":"a.md","content":"# a"}"##;
    let prepared = prepare_write_artifact(&[], args).await.unwrap();
    assert_eq!(prepared.arguments_json, args);
    assert_eq!(prepared.attachment, None);
    let err = prepare_write_artifact(&[], r#"{"name":"a","content":"a\u0000b"}"#)
        .await
        .unwrap_err();
    assert!(err.contains("unsupported"), "{err}");
}
