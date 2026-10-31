//! The push path of the Git endpoint (design D5/D6): spool the body, fetch the mirror,
//! quarantine the pack, apply the ref rules and the push checks, record the push and forward it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use df_proto::gateway::v1::{GitRoute, PushVerdict, RefUpdate};
use http_body_util::{BodyExt, Full};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HeaderValue};
use hyper::{Response, StatusCode};
use tokio::sync::Semaphore;
use uuid::Uuid;

use super::mirror::{MirrorError, Mirrors};
use super::protocol::{
    ReceivePackRequest, forge_report, parse_receive_pack_request, report_status,
    report_status_requested, side_band_requested,
};
use super::quarantine::{MirrorAncestry, QuarantineError, quarantine};
use super::refs::{Ancestry, Reason, RefCommand, RefScope, RefVerdict, check_refs};
use super::server::{Body, BoxError, ForgeTarget, GitRequest, Upstream, text};
use super::spool::{BODY_IDLE_TIMEOUT, SpoolError, SpoolFailure, Spooled, spool};
use super::upstream::{INVALID_URL, REJECTED, UNREACHABLE};

/// Content type of a `report-status` answer.
const RESULT_CONTENT_TYPE: &str = "application/x-git-receive-pack-result";
/// A single allocation while resolving the deltas of a pushed pack is bounded by this multiple
/// of the maximum push size (deltas expand compressed objects).
const OBJECT_ALLOC_FACTOR: u64 = 4;
/// Quarantine and check jobs running at the same time on the blocking pool.
const MAX_CONCURRENT_CHECKS: usize = 4;
/// Upper bound of fetching the mirror for, waiting for, quarantining and checking one push.
const CHECK_DEADLINE: Duration = Duration::from_secs(600);
/// Reason for every ref of a push that carries push options; they are never forwarded.
const PUSH_OPTIONS_NOT_ALLOWED: &str = "df:push-options-not-allowed";
const PUSH_TOO_LARGE: &str = "df:push-too-large";
const BODY_TIMED_OUT: &str = "df:request-timeout";
const BODY_UNREADABLE: &str = "df:unreadable-push";
/// A failure on the gateway's side, not caused by the push.
const GATEWAY_ERROR: &str = "df:gateway-error";
const INVALID_PACK: &str = "df:invalid-pack";
const MALFORMED_PUSH: &str = "df:malformed-push";
const FORGE_UNAVAILABLE: &str = "df:forge-unavailable";
const CHECK_FAILED: &str = "df:check-failed";
/// Largest forge answer to a push read for its `report-status` (report plus hook output).
// ponytail: buffered to read the verdict; an answer above this fails the push even if the forge
// took it. Tee the stream instead if hooks ever print more.
const MAX_FORGE_ANSWER_BYTES: usize = 4 * 1024 * 1024;
/// Reason of a ref the forge refused (`ng`), or of every ref if the whole push failed there.
const FORGE_REFUSED: &str = "df:forge-refused";
/// Time the forge has to send its whole answer to a push (it runs its hooks first).
const FORGE_ANSWER_DEADLINE: Duration = Duration::from_secs(300);

/// A push that failed before its ref verdicts: the answer, and the reason to record it with.
struct Rejection {
    response: Box<Response<Body>>,
    reason: &'static str,
    verdict: PushVerdict,
}

impl Rejection {
    fn new(status: StatusCode, message: &str, reason: &'static str) -> Self {
        Self {
            response: Box::new(text(status, message)),
            reason,
            verdict: PushVerdict::Rejected,
        }
    }

    /// The forge failed before the checks; recorded as `UPSTREAM_FAILED`.
    fn upstream(status: StatusCode, message: &str) -> Self {
        Self {
            verdict: PushVerdict::UpstreamFailed,
            ..Self::new(status, message, FORGE_UNAVAILABLE)
        }
    }
}

/// Checks of a push beyond the ref rules (design D5), run only if the ref rules passed.
pub trait PushChecks: Send + Sync {
    /// One entry per command, in command order: `None` accepts it, `Some` rejects it with a
    /// `df:` reason. `objects` is the quarantine's objects directory (pushed pack plus the
    /// mirror as alternate), `None` for a push without a pack. Runs on the blocking pool.
    fn check(
        &self,
        route: &GitRoute,
        commands: &[RefCommand],
        objects: Option<&Path>,
    ) -> Vec<Option<&'static str>>;
}

/// Accepts every command.
pub struct NoPushChecks;

impl PushChecks for NoPushChecks {
    fn check(
        &self,
        _: &GitRoute,
        commands: &[RefCommand],
        _: Option<&Path>,
    ) -> Vec<Option<&'static str>> {
        vec![None; commands.len()]
    }
}

/// One state of a push, stored with the control plane (design D6).
#[derive(Debug, Clone, PartialEq)]
pub struct PushRecord {
    /// `UUIDv7`; the same for every state of one push.
    pub push_id: Uuid,
    pub run_id: String,
    /// One entry per command, in command order.
    pub refs: Vec<RefUpdate>,
    pub verdict: PushVerdict,
    /// Size of the push request body.
    pub bytes: u64,
    /// From the start of the push to this verdict.
    pub duration: Duration,
}

/// Failure to store a push record.
#[derive(Debug, thiserror::Error)]
#[error("storing the push record failed: {0}")]
pub struct RecordError(pub String);

/// Stores push records with the control plane.
#[async_trait]
pub trait PushRecorder: Send + Sync {
    async fn record(&self, record: PushRecord) -> Result<(), RecordError>;
}

/// Stores nothing.
pub struct NoRecorder;

#[async_trait]
impl PushRecorder for NoRecorder {
    async fn record(&self, _: PushRecord) -> Result<(), RecordError> {
        Ok(())
    }
}

/// Settings and collaborators of the push path.
pub struct Push {
    mirrors: Arc<Mirrors>,
    spool_dir: PathBuf,
    quarantine_dir: PathBuf,
    max_bytes: u64,
    idle: Duration,
    checks: Arc<dyn PushChecks>,
    recorder: Arc<dyn PushRecorder>,
    jobs: Arc<Semaphore>,
}

impl Push {
    /// Pushes checked against `mirrors`, spooled and quarantined under `work_dir`, of at most
    /// `max_bytes` request body. Removes spool and quarantine files left by an earlier process.
    pub fn new(mirrors: Arc<Mirrors>, work_dir: &Path, max_bytes: u64) -> Self {
        let spool_dir = work_dir.join("spool");
        let quarantine_dir = work_dir.join("quarantine");
        for dir in [&spool_dir, &quarantine_dir] {
            remove_leftovers(dir);
        }
        Self {
            mirrors,
            spool_dir,
            quarantine_dir,
            max_bytes,
            idle: BODY_IDLE_TIMEOUT,
            checks: Arc::new(NoPushChecks),
            recorder: Arc::new(NoRecorder),
            jobs: Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS)),
        }
    }

    /// Overrides how long the push body may stall ([`BODY_IDLE_TIMEOUT`]).
    #[must_use]
    pub fn with_idle_timeout(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    #[must_use]
    pub fn with_checks(mut self, checks: Arc<dyn PushChecks>) -> Self {
        self.checks = checks;
        self
    }

    #[must_use]
    pub fn with_recorder(mut self, recorder: Arc<dyn PushRecorder>) -> Self {
        self.recorder = recorder;
        self
    }

    /// Answers a `git-receive-pack` POST of the run `run_id` to `forge`.
    pub(crate) async fn handle(
        &self,
        upstream: &dyn Upstream,
        forge: ForgeTarget<'_>,
        run_id: &str,
        mut request: GitRequest,
    ) -> Response<Body> {
        let started = Instant::now();
        let body = std::mem::replace(&mut request.body, empty());
        let spooled = match spool(body, &self.spool_dir, self.max_bytes, self.idle).await {
            Ok(spooled) => spooled,
            Err(failure) => return self.spool_failed(failure, run_id, started).await,
        };
        if spooled.is_probe() {
            return forward(upstream, forge, request, spooled).await;
        }
        let parsed = match parse_receive_pack_request(spooled.head(), spooled.len()) {
            Ok(parsed) => parsed,
            Err(error) => {
                let entry = Entry::new(run_id, &[], spooled.len(), started);
                self.reject(&entry, Reason::Check(MALFORMED_PUSH)).await;
                return text(
                    StatusCode::BAD_REQUEST,
                    &format!("{MALFORMED_PUSH}: {error}"),
                );
            }
        };
        let entry = Entry::new(run_id, &parsed.commands, spooled.len(), started);
        if parsed.capabilities.iter().any(|c| c == "push-options") {
            tracing::info!(task_key = %request.task_key, "push rejected: push options");
            let verdicts = self
                .reject(&entry, Reason::Check(PUSH_OPTIONS_NOT_ALLOWED))
                .await;
            return report(&parsed, &verdicts);
        }
        let (spooled, verdicts) = match self.verify(forge, &parsed, spooled).await {
            Ok(checked) => checked,
            Err(rejection) => {
                let verdicts = entry.rejected(Reason::Check(rejection.reason));
                self.store(entry.record(&verdicts, rejection.verdict))
                    .await
                    .ok();
                return *rejection.response;
            }
        };
        if verdicts.iter().any(|verdict| !verdict.is_accepted()) {
            tracing::info!(task_key = %request.task_key, "push rejected by the ref rules or checks");
            self.store(entry.record(&verdicts, PushVerdict::Rejected))
                .await
                .ok();
            return report(&parsed, &verdicts);
        }
        if let Err(error) = self
            .store(entry.record(&verdicts, PushVerdict::Forwarding))
            .await
        {
            tracing::warn!(%error, task_key = %request.task_key, "push rejected: record unavailable");
            return report(&parsed, &reject_all(&verdicts, Reason::AuditUnavailable));
        }
        let response = forward(upstream, forge, request, spooled).await;
        let (response, verdict, verdicts) =
            forge_verdict(response, &parsed.capabilities, verdicts).await;
        self.store(entry.record(&verdicts, verdict)).await.ok();
        response
    }

    /// The answer to a body that could not be spooled; the push is recorded as rejected with
    /// the commands found in the part that was read.
    async fn spool_failed(
        &self,
        failure: SpoolFailure,
        run_id: &str,
        started: Instant,
    ) -> Response<Body> {
        let (status, reason, message) = match &failure.error {
            SpoolError::TooLarge { max_bytes } => {
                tracing::warn!(
                    max_bytes,
                    "push rejected: larger than the maximum push size"
                );
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    PUSH_TOO_LARGE,
                    format!("{PUSH_TOO_LARGE}: the push exceeds {max_bytes} bytes"),
                )
            }
            SpoolError::TimedOut => (
                StatusCode::REQUEST_TIMEOUT,
                BODY_TIMED_OUT,
                format!("{BODY_TIMED_OUT}: the request body timed out"),
            ),
            SpoolError::Body(error) => {
                tracing::debug!(%error, "reading the push body failed");
                (
                    StatusCode::BAD_REQUEST,
                    BODY_UNREADABLE,
                    format!("{BODY_UNREADABLE}: reading the push failed"),
                )
            }
            SpoolError::Io(error) => {
                tracing::error!(%error, "spooling the push failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    GATEWAY_ERROR,
                    format!("{GATEWAY_ERROR}: spooling the push failed"),
                )
            }
        };
        let commands = parse_receive_pack_request(&failure.head, u64::MAX)
            .map(|parsed| parsed.commands)
            .unwrap_or_default();
        let entry = Entry::new(run_id, &commands, failure.read, started);
        self.reject(&entry, Reason::Check(reason)).await;
        text(status, &message)
    }

    /// Records the push of `entry` as rejected with `reason` for every command.
    async fn reject(&self, entry: &Entry<'_>, reason: Reason) -> Vec<RefVerdict> {
        let verdicts = entry.rejected(reason);
        self.store(entry.record(&verdicts, PushVerdict::Rejected))
            .await
            .ok();
        verdicts
    }

    /// Fetches the mirror, then applies ref rules and checks on the blocking pool, all within
    /// [`CHECK_DEADLINE`].
    async fn verify(
        &self,
        forge: ForgeTarget<'_>,
        parsed: &ReceivePackRequest,
        spooled: Spooled,
    ) -> Result<(Spooled, Vec<RefVerdict>), Rejection> {
        let deadline = tokio::time::Instant::now() + CHECK_DEADLINE;
        let failed = || {
            Rejection::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("{CHECK_FAILED}: checking the push failed"),
                CHECK_FAILED,
            )
        };
        let mirror = tokio::time::timeout_at(deadline, self.mirrors.fetch(forge))
            .await
            .map_err(|_elapsed| {
                tracing::error!("push check deadline exceeded while fetching the mirror");
                failed()
            })?
            .map_err(mirror_rejection)?;
        let job = Verify {
            mirror: mirror.path().to_owned(),
            quarantine_dir: self.quarantine_dir.clone(),
            scope: RefScope {
                task_branch: forge.route.task_branch.clone(),
                wip_branch: forge.route.wip_branch.clone(),
            },
            commands: parsed.commands.clone(),
            pack_offset: parsed.pack_offset,
            route: forge.route.clone(),
            checks: self.checks.clone(),
            alloc_limit: usize::try_from(self.max_bytes.saturating_mul(OBJECT_ALLOC_FACTOR))
                .unwrap_or(usize::MAX),
            interrupt: Arc::new(AtomicBool::new(false)),
        };
        // Dropping this future (client gone) or the deadline aborts a running indexing.
        let _interrupt = InterruptOnDrop(job.interrupt.clone());
        let blocking = run_limited(&self.jobs, move || {
            let outcome = job.run(&spooled);
            (spooled, outcome)
        });
        let joined = tokio::time::timeout_at(deadline, blocking)
            .await
            .unwrap_or_else(|_elapsed| Err("push check deadline exceeded".into()));
        match joined {
            Ok((spooled, Ok(verdicts))) => Ok((spooled, verdicts)),
            Ok((_, Err(QuarantineError::Pack(error)))) => {
                tracing::info!(%error, "push rejected: invalid pack");
                Err(Rejection::new(
                    StatusCode::BAD_REQUEST,
                    &format!("{INVALID_PACK}: the pushed pack is invalid"),
                    INVALID_PACK,
                ))
            }
            Ok((_, Err(error))) => {
                tracing::error!(%error, "checking a push failed");
                Err(failed())
            }
            Err(error) => {
                tracing::error!(%error, "push check task failed");
                Err(failed())
            }
        }
    }

    async fn store(&self, record: PushRecord) -> Result<(), RecordError> {
        let result = self.recorder.record(record).await;
        if let Err(error) = &result {
            tracing::warn!(%error, "push record not stored");
        }
        result
    }
}

/// The blocking part of a push check: quarantine, ref rules, push checks.
struct Verify {
    mirror: PathBuf,
    quarantine_dir: PathBuf,
    scope: RefScope,
    commands: Vec<RefCommand>,
    pack_offset: Option<usize>,
    route: GitRoute,
    checks: Arc<dyn PushChecks>,
    alloc_limit: usize,
    interrupt: Arc<AtomicBool>,
}

impl Verify {
    fn run(&self, spooled: &Spooled) -> Result<Vec<RefVerdict>, QuarantineError> {
        let Some(offset) = self.pack_offset else {
            return self.verdicts(&MirrorAncestry::open(&self.mirror)?, None);
        };
        let pack = spooled.reader_at(u64::try_from(offset).unwrap_or(u64::MAX))?;
        let quarantined = quarantine(
            &self.mirror,
            pack,
            &self.quarantine_dir,
            self.alloc_limit,
            &self.interrupt,
        )?;
        self.verdicts(&quarantined, Some(&quarantined.objects_dir()))
    }

    fn verdicts<A>(
        &self,
        ancestry: &A,
        objects: Option<&Path>,
    ) -> Result<Vec<RefVerdict>, QuarantineError>
    where
        A: Ancestry<Error = QuarantineError>,
    {
        let verdicts = check_refs(&self.scope, &self.commands, ancestry)?;
        if verdicts.iter().any(|verdict| !verdict.is_accepted()) {
            return Ok(verdicts);
        }
        let reasons = self.checks.check(&self.route, &self.commands, objects);
        if reasons.len() != verdicts.len() {
            tracing::error!(
                "push checks returned a verdict count that does not match the commands"
            );
            return Ok(reject_all(&verdicts, Reason::Check(CHECK_FAILED)));
        }
        let mut verdicts: Vec<RefVerdict> = verdicts
            .into_iter()
            .zip(reasons)
            .map(|(verdict, reason)| RefVerdict {
                reason: reason.map(Reason::Check),
                ..verdict
            })
            .collect();
        if verdicts.iter().any(|verdict| !verdict.is_accepted()) {
            for verdict in &mut verdicts {
                verdict.reason.get_or_insert(Reason::OtherRefRejected);
            }
        }
        Ok(verdicts)
    }
}

/// The answer to a push whose mirror fetch failed.
fn mirror_rejection(error: MirrorError) -> Rejection {
    tracing::warn!(%error, "mirror fetch before a push failed");
    match error {
        MirrorError::Unreachable(_) => {
            Rejection::upstream(StatusCode::SERVICE_UNAVAILABLE, UNREACHABLE)
        }
        MirrorError::Rejected(_) => Rejection::upstream(StatusCode::BAD_GATEWAY, REJECTED),
        MirrorError::InvalidUrl => Rejection::upstream(StatusCode::BAD_GATEWAY, INVALID_URL),
        MirrorError::ForgeStatus(status) => Rejection::upstream(
            StatusCode::BAD_GATEWAY,
            &format!("{FORGE_UNAVAILABLE}: the forge answered the fetch with HTTP {status}"),
        ),
        MirrorError::Repository(_) | MirrorError::Fetch(_) | MirrorError::Task(_) => {
            Rejection::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("{GATEWAY_ERROR}: fetching the forge repository failed"),
                GATEWAY_ERROR,
            )
        }
    }
}

/// Sets its flag when dropped.
struct InterruptOnDrop(Arc<AtomicBool>);

impl Drop for InterruptOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Runs `job` on the blocking pool once a permit of `jobs` is free; the permit is held until
/// `job` returns, even if the caller stops waiting.
async fn run_limited<T, F>(jobs: &Arc<Semaphore>, job: F) -> Result<T, BoxError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = jobs.clone().acquire_owned().await?;
    let joined = tokio::task::spawn_blocking(move || {
        let outcome = job();
        drop(permit);
        outcome
    })
    .await?;
    Ok(joined)
}

/// What every record of one push shares.
struct Entry<'a> {
    push_id: Uuid,
    run_id: &'a str,
    commands: &'a [RefCommand],
    bytes: u64,
    started: Instant,
}

impl Entry<'_> {
    fn new<'a>(
        run_id: &'a str,
        commands: &'a [RefCommand],
        bytes: u64,
        started: Instant,
    ) -> Entry<'a> {
        Entry {
            push_id: Uuid::now_v7(),
            run_id,
            commands,
            bytes,
            started,
        }
    }

    /// Every command rejected with `reason`.
    fn rejected(&self, reason: Reason) -> Vec<RefVerdict> {
        self.commands
            .iter()
            .map(|command| RefVerdict {
                name: command.name.clone(),
                reason: Some(reason),
            })
            .collect()
    }

    fn record(&self, verdicts: &[RefVerdict], verdict: PushVerdict) -> PushRecord {
        let refs = self
            .commands
            .iter()
            .zip(verdicts)
            .map(|(command, ref_verdict)| RefUpdate {
                name: command.name.clone(),
                old_sha: command.old.to_string(),
                new_sha: command.new.to_string(),
                verdict: i32::from(verdict),
                reason: ref_verdict
                    .reason
                    .map(|reason| reason.as_str().to_owned())
                    .unwrap_or_default(),
            })
            .collect();
        PushRecord {
            push_id: self.push_id,
            run_id: self.run_id.to_owned(),
            refs,
            verdict,
            bytes: self.bytes,
            duration: self.started.elapsed(),
        }
    }
}

fn reject_all(verdicts: &[RefVerdict], reason: Reason) -> Vec<RefVerdict> {
    verdicts
        .iter()
        .map(|verdict| RefVerdict {
            name: verdict.name.clone(),
            reason: Some(reason),
        })
        .collect()
}

/// A `report-status` answer; empty if the client requested none.
fn report(parsed: &ReceivePackRequest, verdicts: &[RefVerdict]) -> Response<Body> {
    let body = if report_status_requested(&parsed.capabilities) {
        report_status(verdicts, side_band_requested(&parsed.capabilities))
    } else {
        Vec::new()
    };
    let mut response = Response::new(
        Full::new(Bytes::from(body))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(RESULT_CONTENT_TYPE));
    response
}

/// The verdict of a forwarded push, read from the forge's `report-status` if the client requested
/// one (a forge answers a refused ref with `200` and `ng`), else from the HTTP status. The answer
/// is buffered for that and handed to the client unchanged. Refs the forge refused (every ref if
/// the whole push failed) get the reason [`FORGE_REFUSED`].
async fn forge_verdict(
    response: Response<Body>,
    capabilities: &[String],
    verdicts: Vec<RefVerdict>,
) -> (Response<Body>, PushVerdict, Vec<RefVerdict>) {
    if !response.status().is_success() {
        return (response, PushVerdict::UpstreamFailed, verdicts);
    }
    if !report_status_requested(capabilities) {
        return (response, PushVerdict::Accepted, verdicts);
    }
    let side_band = side_band_requested(capabilities);
    let (parts, body) = response.into_parts();
    // Boxed: collecting `Limited<Body>` directly trips rustc's higher-ranked `Send` check.
    let limited = http_body_util::Limited::new(body, MAX_FORGE_ANSWER_BYTES)
        .boxed_unsync()
        .collect();
    let answer = match tokio::time::timeout(FORGE_ANSWER_DEADLINE, limited).await {
        Ok(Ok(collected)) => collected.to_bytes(),
        Ok(Err(error)) => {
            tracing::warn!(%error, "reading the forge's answer to a push failed");
            return forge_answer_failed(verdicts);
        }
        Err(_elapsed) => {
            tracing::warn!("the forge did not finish its answer to a push in time");
            return forge_answer_failed(verdicts);
        }
    };
    let report = forge_report(&answer, side_band);
    let verdict = if report.accepted() {
        PushVerdict::Accepted
    } else {
        tracing::info!(failure = ?report.failure, refused = ?report.refused, "the forge refused the push");
        PushVerdict::UpstreamFailed
    };
    let verdicts = verdicts
        .into_iter()
        .map(|verdict| {
            if report.failure.is_some() || report.refused.contains(&verdict.name) {
                RefVerdict {
                    reason: Some(Reason::Check(FORGE_REFUSED)),
                    ..verdict
                }
            } else {
                verdict
            }
        })
        .collect();
    let body = Full::new(answer)
        .map_err(|never| match never {})
        .boxed_unsync();
    (Response::from_parts(parts, body), verdict, verdicts)
}

fn forge_answer_failed(
    verdicts: Vec<RefVerdict>,
) -> (Response<Body>, PushVerdict, Vec<RefVerdict>) {
    let message = format!("{FORGE_UNAVAILABLE}: reading the forge's answer failed");
    (
        text(StatusCode::BAD_GATEWAY, &message),
        PushVerdict::UpstreamFailed,
        verdicts,
    )
}

/// Sends the spooled body upstream; the spool file goes away with the body.
async fn forward(
    upstream: &dyn Upstream,
    forge: ForgeTarget<'_>,
    mut request: GitRequest,
    spooled: Spooled,
) -> Response<Body> {
    let len = spooled.len();
    request.body = spooled
        .into_body()
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed_unsync();
    request
        .headers
        .insert(CONTENT_LENGTH, HeaderValue::from(len));
    upstream.forward(forge, request).await
}

fn empty() -> Body {
    Full::new(Bytes::new())
        .map_err(|never| match never {})
        .boxed_unsync()
}

/// Removes `dir`; it is created again on demand.
fn remove_leftovers(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!(%error, dir = %dir.display(), "removing leftover push files failed");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[tokio::test]
    async fn a_second_check_job_waits_for_the_permit_of_the_first() {
        let jobs = Arc::new(Semaphore::new(1));
        let running = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();

        let first = {
            let (jobs, running) = (jobs.clone(), running.clone());
            let job = move || {
                running.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                running.fetch_sub(1, Ordering::SeqCst)
            };
            tokio::spawn(async move { run_limited(&jobs, job).await })
        };
        started_rx.await.unwrap();
        let second = {
            let (jobs, running) = (jobs.clone(), running.clone());
            tokio::spawn(
                async move { run_limited(&jobs, move || running.load(Ordering::SeqCst)).await },
            )
        };
        tokio::task::yield_now().await;

        assert_eq!(jobs.available_permits(), 0);
        assert!(!second.is_finished());
        release_tx.send(()).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), 1);
        assert_eq!(
            second.await.unwrap().unwrap(),
            0,
            "ran next to the first job"
        );
        assert_eq!(jobs.available_permits(), 1);
    }

    #[test]
    fn dropping_the_guard_sets_the_interrupt() {
        let flag = Arc::new(AtomicBool::new(false));

        drop(InterruptOnDrop(flag.clone()));

        assert!(flag.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn leftover_spool_and_quarantine_files_are_removed_on_start() {
        let work = tempfile::tempdir().unwrap();
        for dir in ["spool", "quarantine/q-1"] {
            std::fs::create_dir_all(work.path().join(dir)).unwrap();
        }
        std::fs::write(work.path().join("spool/push-1"), b"pack").unwrap();
        std::fs::write(work.path().join("keep"), b"mirror").unwrap();
        let mirrors = Mirrors::new(
            work.path(),
            super::super::upstream::ForgeConnector::new().unwrap(),
        );

        Push::new(Arc::new(mirrors), work.path(), 1024);

        assert!(!work.path().join("spool").exists());
        assert!(!work.path().join("quarantine").exists());
        assert!(work.path().join("keep").exists());
    }
}
