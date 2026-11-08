//! `write_artifact` with `path`: the runtime reads the file itself (spec `factory-mcp-server`
//! "Artifact files read by the runtime", design D3, D4).
//!
//! The path comes from the harness, so everything it can influence is untrusted: the file is
//! opened with `openat2` beneath its root, refusing every symlink, and only then read.

use std::io::Read;
use std::path::{Path, PathBuf};

use df_artifact_types::MAX_ARTIFACT_BYTES;
use serde_json::Value;

/// A `write_artifact` call after the runtime's preparation.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparedWrite {
    /// The arguments to send, without `path`.
    pub arguments_json: String,
    /// The file content read for `path`.
    pub attachment: Option<Vec<u8>>,
}

/// Resolves `path` of a `write_artifact` call and checks the content type early. A call
/// without `path` is returned as it is (its `content` is checked). The error is the text for the
/// model; nothing is sent when it is returned.
pub async fn prepare_write_artifact(
    roots: &[PathBuf],
    arguments_json: &str,
) -> Result<PreparedWrite, String> {
    let Ok(Value::Object(mut args)) = serde_json::from_str::<Value>(arguments_json) else {
        // The control plane reports invalid arguments.
        return Ok(PreparedWrite {
            arguments_json: arguments_json.to_string(),
            attachment: None,
        });
    };
    if let Some(content) = args.get("content").and_then(Value::as_str) {
        check_content_type(content.as_bytes())?;
    }
    let Some(path) = args.get("path").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(PreparedWrite {
            arguments_json: arguments_json.to_string(),
            attachment: None,
        });
    };
    let roots = roots.to_vec();
    let bytes = tokio::task::spawn_blocking(move || read_artifact_file(&roots, &path))
        .await
        .map_err(|e| format!("reading the file failed: {e}"))??;
    check_content_type(&bytes)?;
    args.remove("path");
    Ok(PreparedWrite {
        arguments_json: Value::Object(args).to_string(),
        attachment: Some(bytes),
    })
}

/// Reads the regular file `path` if it lies beneath one of `roots` without passing a symlink.
pub fn read_artifact_file(roots: &[PathBuf], path: &str) -> Result<Vec<u8>, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    let Some((root, rest)) = roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok().map(|rest| (root, rest)))
    else {
        let allowed: Vec<_> = roots.iter().map(|r| r.display().to_string()).collect();
        return Err(format!(
            "path must be inside one of {}; copy the file there first",
            allowed.join(" or ")
        ));
    };
    if rest.as_os_str().is_empty() {
        return Err("path is not a regular file".into());
    }
    let file = open::open_beneath(root, rest)?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("cannot inspect the file: {e}"))?;
    if !metadata.is_file() {
        return Err("path is not a regular file".into());
    }
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "the file is {} bytes, over the limit of {MAX_ARTIFACT_BYTES} bytes",
            metadata.len()
        ));
    }
    // One byte more than the limit, so a file that grows after the check is still caught.
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read the file: {e}"))?;
    if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "the file is over the limit of {MAX_ARTIFACT_BYTES} bytes"
        ));
    }
    Ok(bytes)
}

/// Rejects content that is neither text nor a supported image; the control plane decides again.
fn check_content_type(bytes: &[u8]) -> Result<(), String> {
    df_artifact_types::sniff_kind(bytes)
        .map(|_kind| ())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod open {
    use std::fs::File;
    use std::path::Path;

    use rustix::fs::{CWD, Mode, OFlags, ResolveFlags, openat2};
    use rustix::io::Errno;

    /// Opens `rest` below the directory `root` for reading. Symlinks anywhere in `rest`, magic
    /// links and any escape from `root` fail.
    pub(super) fn open_beneath(root: &Path, rest: &Path) -> Result<File, String> {
        let root_fd = rustix::fs::openat(
            CWD,
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        // NONBLOCK: a FIFO must not block the open; the type check rejects it afterwards.
        let fd = openat2(
            &root_fd,
            rest,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|errno| match errno {
            Errno::LOOP | Errno::XDEV => {
                "path passes through a symlink or leaves the allowed directory".to_string()
            }
            Errno::NOENT => "path does not exist".to_string(),
            Errno::NOSYS => {
                "this kernel lacks openat2; use `content` instead of `path`".to_string()
            }
            other => format!("cannot open the file: {other}"),
        })?;
        Ok(File::from(fd))
    }
}

#[cfg(not(target_os = "linux"))]
mod open {
    use std::fs::File;
    use std::path::Path;

    pub(super) fn open_beneath(_root: &Path, _rest: &Path) -> Result<File, String> {
        Err("reading files for artifacts needs Linux; use `content` instead of `path`".into())
    }
}

#[cfg(test)]
mod tests;
