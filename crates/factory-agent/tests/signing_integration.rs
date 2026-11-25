//! A real `git commit` signed through the `git-sign` shim, the runtime socket and a test signer
//! (`persona-commit-signing`, design D1).

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "helper functions of an integration test"
)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use base64::Engine;
use ssh_key::private::{Ed25519Keypair, KeypairData};
use ssh_key::{HashAlg, LineEnding, PrivateKey, SshSig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Cargo sets `CARGO_BIN_EXE_*` at compile time; Bazel passes the path in `DF_TEST_BIN`.
fn binary() -> PathBuf {
    let path = option_env!("CARGO_BIN_EXE_factory-agent")
        .map(PathBuf::from)
        .or_else(|| std::env::var("DF_TEST_BIN").ok().map(PathBuf::from))
        .expect("binary path (CARGO_BIN_EXE_factory-agent or DF_TEST_BIN)");
    // The shim runs in the repository, so a relative path (Bazel) must not depend on the cwd.
    std::path::absolute(path).expect("absolute binary path")
}

fn persona_key() -> PrivateKey {
    PrivateKey::new(
        KeypairData::Ed25519(Ed25519Keypair::from_seed(&[7; 32])),
        "",
    )
    .unwrap()
}

async fn read_frame(stream: &mut UnixStream) -> serde_json::Value {
    let mut len = [0_u8; 4];
    stream.read_exact(&mut len).await.unwrap();
    let mut body = vec![0_u8; u32::from_be_bytes(len) as usize];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn write_frame(stream: &mut UnixStream, value: &serde_json::Value) {
    let body = serde_json::to_vec(value).unwrap();
    stream
        .write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&body).await.unwrap();
}

/// The control plane's end behind the runtime socket: signs every request with the persona key, or
/// refuses it with `refusal`.
fn spawn_signer(socket: &Path, refusal: Option<&'static str>) {
    let listener = UnixListener::bind(socket).unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_frame(&mut stream).await;
            assert_eq!(request["type"], "git_sign");
            let payload = base64::engine::general_purpose::STANDARD
                .decode(request["payload"].as_str().unwrap())
                .unwrap();
            let response = if let Some(reason) = refusal {
                serde_json::json!({
                    "type": "git_sign", "signature": null, "refused_reason": reason
                })
            } else {
                let signature = persona_key()
                    .sign("git", HashAlg::Sha512, &payload)
                    .unwrap()
                    .to_pem(LineEnding::LF)
                    .unwrap();
                serde_json::json!({
                    "type": "git_sign", "signature": signature, "refused_reason": null
                })
            };
            write_frame(&mut stream, &response).await;
        }
    });
}

struct Repo {
    dir: tempfile::TempDir,
    socket: PathBuf,
}

impl Repo {
    /// A repository configured like the runtime configures the workspace.
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("df-git-sign");
        std::fs::write(
            &shim,
            format!("#!/bin/sh\nexec {} git-sign \"$@\"\n", binary().display()),
        )
        .unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let repo = Self {
            socket: dir.path().join("runtime.sock"),
            dir,
        };
        std::fs::create_dir(repo.path()).unwrap();
        repo.git(&["init", "-q", "-b", "main"]).await;
        let public = persona_key().public_key().to_openssh().unwrap();
        for (key, value) in [
            ("user.name", "Linus"),
            ("user.email", "linus@example.org"),
            ("gpg.format", "ssh"),
            ("gpg.ssh.program", shim.to_str().unwrap()),
            ("user.signingKey", &format!("key::{public}")),
            ("commit.gpgSign", "true"),
        ] {
            repo.git(&["config", key, value]).await;
        }
        repo
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn command(&self, args: &[&str]) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("git");
        cmd.current_dir(self.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("DF_RUNTIME_SOCK", &self.socket)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    async fn git(&self, args: &[&str]) -> String {
        let out = self.command(args).output().await.unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

/// The signed payload (the commit without its `gpgsig` header) and the armored signature of
/// `HEAD`.
async fn payload_and_signature(repo: &Repo) -> (Vec<u8>, String) {
    let raw = repo.git(&["cat-file", "commit", "HEAD"]).await;
    let (mut payload, mut signature) = (String::new(), String::new());
    let mut in_signature = false;
    for line in raw.split_inclusive('\n') {
        if let Some(first) = line.strip_prefix("gpgsig ") {
            in_signature = true;
            signature.push_str(first);
        } else if in_signature && line.starts_with(' ') {
            signature.push_str(&line[1..]);
        } else {
            in_signature = false;
            payload.push_str(line);
        }
    }
    (payload.into_bytes(), signature)
}

#[tokio::test]
async fn signing_integration_a_commit_is_signed_and_verifies_with_the_persona_key() {
    let repo = Repo::new().await;
    spawn_signer(&repo.socket, None);

    repo.git(&["commit", "--allow-empty", "-m", "Fix login"])
        .await;

    let (payload, signature) = payload_and_signature(&repo).await;
    assert!(!signature.is_empty(), "the commit carries a gpgsig header");
    let sig = SshSig::from_pem(&signature).unwrap();
    persona_key()
        .public_key()
        .verify("git", &payload, &sig)
        .unwrap();

    // The same commit verifies with ssh-keygen, where it is installed.
    if std::process::Command::new("ssh-keygen")
        .arg("-?")
        .output()
        .is_ok()
    {
        let allowed = repo.dir.path().join("allowed_signers");
        std::fs::write(
            &allowed,
            format!(
                "linus@example.org {}\n",
                persona_key().public_key().to_openssh().unwrap()
            ),
        )
        .unwrap();
        let verified = repo
            .command(&[
                "-c",
                "gpg.ssh.program=ssh-keygen",
                "-c",
                &format!("gpg.ssh.allowedSignersFile={}", allowed.display()),
                "verify-commit",
                "HEAD",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            verified.status.success(),
            "{}",
            String::from_utf8_lossy(&verified.stderr)
        );
    }
}

#[tokio::test]
async fn signing_integration_a_refused_signature_fails_the_commit_with_the_reason() {
    let repo = Repo::new().await;
    spawn_signer(
        &repo.socket,
        Some("author does not match the persona identity"),
    );

    let out = repo
        .command(&["commit", "--allow-empty", "-m", "Fix login"])
        .output()
        .await
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("author does not match the persona identity"),
        "{stderr}"
    );
    let head = repo
        .command(&["rev-parse", "--verify", "HEAD"])
        .output()
        .await
        .unwrap();
    assert!(!head.status.success(), "no commit was created");
}

#[tokio::test]
async fn signing_integration_a_missing_runtime_socket_fails_the_commit() {
    let repo = Repo::new().await;

    let out = repo
        .command(&["commit", "--allow-empty", "-m", "Fix login"])
        .output()
        .await
        .unwrap();

    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("signing is unavailable"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
