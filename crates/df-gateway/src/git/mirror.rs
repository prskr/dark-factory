//! Bare mirror repositories of the forge repositories, updated with a gix fetch (design D5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use gix_error::ErrorExt as _;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle;

use super::server::ForgeTarget;
use super::upstream::{ForgeConnector, basic};

mod transport;

/// Fetches every branch of the forge into the mirror, pruning nothing.
const REFSPEC: &str = "+refs/heads/*:refs/heads/*";
/// Longest time one fetch may take; then its reads fail and gix is interrupted.
pub const FETCH_DEADLINE: Duration = Duration::from_secs(300);

/// The mirrors under one directory, one per upstream URL.
pub struct Mirrors {
    dir: PathBuf,
    connector: ForgeConnector,
    locks: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

/// A mirror after a successful fetch.
#[derive(Debug, Clone)]
pub struct MirrorRepo {
    path: PathBuf,
}

impl MirrorRepo {
    /// Path of the bare repository.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Why a mirror could not be fetched.
#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("the route has no usable upstream URL")]
    InvalidUrl,
    #[error("mirror repository: {0}")]
    Repository(String),
    #[error("fetch from the forge failed: {0}")]
    Fetch(String),
    /// Connect, TLS or a read failed or timed out.
    #[error("forge unreachable: {0}")]
    Unreachable(String),
    /// The forge answered 401 or 403.
    #[error("the forge rejected the credential: {0}")]
    Rejected(String),
    /// The forge answered another non-2xx status.
    #[error("the forge answered HTTP {0}")]
    ForgeStatus(u16),
    #[error("fetch task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

impl Mirrors {
    /// Mirrors under `dir`, fetched through `connector`.
    pub fn new(dir: impl Into<PathBuf>, connector: ForgeConnector) -> Self {
        Self {
            dir: dir.into(),
            connector,
            locks: Mutex::default(),
        }
    }

    /// Creates the mirror of the route's repository on first use and fetches its branches.
    /// Fetches of the same mirror run one after the other; a fetch whose caller is cancelled
    /// keeps the mirror locked until it ends, at the latest after [`FETCH_DEADLINE`].
    pub async fn fetch(&self, forge: ForgeTarget<'_>) -> Result<MirrorRepo, MirrorError> {
        let url = forge.route.upstream_url.clone();
        let forge_url = self
            .connector
            .forge_url(&url)
            .ok_or(MirrorError::InvalidUrl)?;
        let path = self.dir.join(hex::encode(Sha256::digest(url.as_bytes())));
        let guard = self.lock_for(&path).lock_owned().await;
        let deadline = tokio::time::Instant::now() + FETCH_DEADLINE;
        let http = transport::HyperHttp::new(
            Handle::current(),
            self.connector.clone(),
            forge_url,
            basic(forge.credential),
            deadline,
        );
        let failure = http.failure();
        let interrupt = Arc::new(AtomicBool::new(false));
        let timer = {
            let interrupt = interrupt.clone();
            tokio::spawn(async move {
                tokio::time::sleep_until(deadline).await;
                interrupt.store(true, Ordering::Relaxed);
            })
        };
        let target = path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let result = fetch_blocking(http, &url, &target, &interrupt);
            timer.abort();
            drop(guard);
            result
        })
        .await?;
        match result {
            Ok(()) => Ok(MirrorRepo { path }),
            Err(MirrorError::Fetch(message)) => Err(match failure.get() {
                Some(transport::ForgeFailure::Unreachable) => MirrorError::Unreachable(message),
                Some(transport::ForgeFailure::Rejected) => MirrorError::Rejected(message),
                Some(transport::ForgeFailure::Status(status)) => MirrorError::ForgeStatus(status),
                None => MirrorError::Fetch(message),
            }),
            Err(error) => Err(error),
        }
    }

    fn lock_for(&self, path: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        locks.entry(path.to_owned()).or_default().clone()
    }
}

fn fetch_blocking(
    http: transport::HyperHttp,
    url: &str,
    path: &Path,
    interrupt: &AtomicBool,
) -> Result<(), MirrorError> {
    if !path.join("HEAD").exists() {
        std::fs::create_dir_all(path).map_err(|e| MirrorError::Repository(chain(&e)))?;
        gix::init_bare(path).map_err(|e| MirrorError::Repository(chain(&e)))?;
    }
    // Isolated: no system or user Git configuration (credential helpers, proxies).
    let repo = gix::open_opts(path, gix::open::Options::isolated())
        .map_err(|e| MirrorError::Repository(chain(&e)))?;
    let remote = repo
        .remote_at(url)
        .map_err(|e| MirrorError::Repository(chain(&e)))?
        .with_refspecs([REFSPEC], gix::remote::Direction::Fetch)
        .map_err(|e| MirrorError::Repository(chain(&e)))?;
    let parsed = gix::url::parse(url.as_bytes()).map_err(|e| MirrorError::Fetch(chain(&e)))?;
    let transport = gix_transport::client::blocking_io::http::connect_http(
        http,
        parsed,
        gix_transport::Protocol::V2,
        false,
    );
    let options = gix::remote::ref_map::Options {
        prefix_from_spec_as_filter_on_remote: true,
        ..Default::default()
    };
    remote
        .to_connection_with_transport(transport)
        // On 401 gix would otherwise ask the configured credential helpers.
        .with_credentials(|_| Err(gix_error::message("credential helpers disabled").raise()))
        .prepare_fetch(gix::progress::Discard, options)
        .map_err(|e| MirrorError::Fetch(chain(&e)))?
        .receive(gix::progress::Discard, interrupt)
        .map_err(|e| MirrorError::Fetch(chain(&e)))?;
    Ok(())
}

/// `error` with its sources, `: `-separated.
fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
