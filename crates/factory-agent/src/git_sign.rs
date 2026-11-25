//! `factory-agent git-sign`: the `gpg.ssh.program` of every commit in the workspace (design D1 of
//! `persona-commit-signing`).
//!
//! Git calls it like `ssh-keygen`: `-Y sign -n git -f <keyfile> [-U] <datafile>`. The shim sends
//! the data file to the runtime over the runtime socket and writes the armored signature it gets
//! to `<datafile>.sig`. The key file is ignored: the control plane signs with the persona's
//! current key. It knows nothing else: any other mode (`verify`, `find-principals`, ...) or
//! namespace fails.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use tokio::net::UnixStream;

use crate::runtime_socket::{RuntimeRequest, RuntimeResponse, read_frame, write_frame};

/// How long the shim waits for the runtime: slightly longer than the runtime waits for the
/// control plane ([`crate::sign_relay::SIGN_TIMEOUT`]), so the runtime's reason arrives first.
const RUNTIME_TIMEOUT: Duration = Duration::from_secs(12);

/// Signs the data file named by `args` through the runtime at `socket`; the error is the message
/// for stderr.
pub async fn run(socket: &Path, args: &[OsString]) -> Result<(), String> {
    let data_file = parse_args(args)?;
    let payload = std::fs::read(&data_file)
        .map_err(|e| format!("cannot read {}: {e}", data_file.display()))?;
    let signature = request_signature(socket, &payload, RUNTIME_TIMEOUT).await?;
    let mut signature_file = data_file.into_os_string();
    signature_file.push(".sig");
    std::fs::write(&signature_file, signature)
        .map_err(|e| format!("cannot write {}: {e}", Path::new(&signature_file).display()))
}

/// The data file of `-Y sign -n git -f <keyfile> [-U] <datafile>`.
fn parse_args(args: &[OsString]) -> Result<PathBuf, String> {
    let (mut mode, mut namespace, mut data_file) = (None, None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-Y") => mode = args.next(),
            Some("-n") => namespace = args.next(),
            // The key is ignored; `-U` only says that the key is literal.
            Some("-f") => {
                args.next();
            }
            Some("-U") => {}
            _ => data_file = Some(arg),
        }
    }
    if mode.map(OsString::as_os_str) != Some(OsStr::new("sign")) {
        return Err(format!(
            "unsupported mode {}: only `-Y sign` is supported, signatures are not verified in the sandbox",
            mode.map_or_else(|| "(none)".into(), |m| m.to_string_lossy())
        ));
    }
    if namespace.map(OsString::as_os_str) != Some(OsStr::new("git")) {
        return Err(format!(
            "unsupported namespace {}: only `-n git` is supported",
            namespace.map_or_else(|| "(none)".into(), |n| n.to_string_lossy())
        ));
    }
    data_file
        .map(PathBuf::from)
        .ok_or_else(|| "no file to sign".to_string())
}

async fn request_signature(
    socket: &Path,
    payload: &[u8],
    timeout: Duration,
) -> Result<String, String> {
    let unavailable = |e: &dyn std::fmt::Display| format!("signing is unavailable: {e}");
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|e| unavailable(&e))?;
    let request = RuntimeRequest::GitSign {
        payload: base64::engine::general_purpose::STANDARD.encode(payload),
    };
    write_frame(&mut stream, &request)
        .await
        .map_err(|e| unavailable(&e))?;
    let answer = tokio::time::timeout(timeout, read_frame(&mut stream))
        .await
        .map_err(|_elapsed| {
            unavailable(&format!(
                "the runtime did not answer within {} s",
                timeout.as_secs().max(1)
            ))
        })?;
    match answer.map_err(|e| unavailable(&e))? {
        RuntimeResponse::GitSign {
            signature: Some(signature),
            ..
        } => Ok(signature),
        RuntimeResponse::GitSign {
            refused_reason: Some(reason),
            ..
        } => Err(reason),
        other => Err(unavailable(&format!("unexpected answer {other:?}"))),
    }
}
