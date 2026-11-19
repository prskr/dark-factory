//! The fixture of a test: [`run`] builds the [`World`] (environment, logged-in browser session,
//! database pool, Forgejo clients, unique suffix), runs the body, and tears down every time.
//!
//! Teardown keeps the next test and the next run of the suite from meeting this test's leftovers:
//! it cancels the tasks the test created and the tasks those created (delegation children), so no
//! sandbox keeps running. When the test failed, it writes the screenshot and the console
//! of the browser to `$E2E_ARTIFACTS_DIR/<test name>/` first.

use std::any::Any;
use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use df_core::StatusGroup;
use sqlx::PgPool;
use tokio::task::JoinError;
use uuid::Uuid;

use crate::E2eEnv;
use crate::browser::{Session, cancel_task_over_new_connection, report};
use crate::db;
use crate::forgejo::Forgejo;
use crate::wait::{UI_TIMEOUT, eventually};

/// Directory of the per-test diagnostics unless `E2E_ARTIFACTS_DIR` says otherwise.
const DEFAULT_ARTIFACTS_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/e2e-artifacts");

/// How long the teardown waits for the runs of a cancelled task to end before it looks for the
/// tasks the runs created.
const RUNS_END_TIMEOUT: Duration = Duration::from_secs(60);

/// What a test body can use. Shared with the body through an [`Arc`], so the body runs in a task of
/// its own and the teardown still has the browser, the pool and the recorded tasks.
pub struct World {
    pub env: E2eEnv,
    pub session: Session,
    pub pool: PgPool,
    /// The bot's client for `factory/demo`.
    pub forgejo: Forgejo,
    /// The second user's client for `factory/demo`.
    pub reviewer: Forgejo,
    /// First 8 hex characters of a random id, unique per test run: appended to every name (branch,
    /// file, persona, issue title) the test creates, so a repeated run finds nothing of the last.
    pub suffix: String,
    name: String,
}

impl World {
    async fn start(name: &str) -> Result<Self> {
        let env = E2eEnv::from_env()?;
        let session = Session::start_logged_in(&env).await?;
        let suffix = unique_suffix();
        session.set_description_suffix(&suffix);
        Ok(Self {
            forgejo: Forgejo::new(&env.forgejo_url, &env.forgejo_token)?,
            reviewer: Forgejo::new(&env.forgejo_url, &env.forgejo_reviewer_token)?,
            pool: db::connect(&env.database_url).await?,
            suffix,
            name: name.to_owned(),
            env,
            session,
        })
    }

    /// `base` with the unique suffix: `unique("bump")` is `bump-1a2b3c4d`.
    pub fn unique(&self, base: &str) -> String {
        format!("{base}-{}", self.suffix)
    }

    /// Waits until task `key` has `status`; see [`db::wait_for_status`].
    pub async fn wait_for_status(&self, key: &str, status: df_core::TaskStatus) -> Result<()> {
        db::wait_for_status(&self.pool, key, status).await
    }

    /// Waits until `coordinator` has created the child that `marker` identifies (the marker of
    /// the prompt it was created with); returns its key.
    pub async fn child_of(&self, coordinator: &str, marker: &str) -> Result<String> {
        eventually(
            &format!("{coordinator} to create the child with {marker}"),
            crate::wait::RUN_TIMEOUT,
            || db::child_described(&self.pool, coordinator, marker),
        )
        .await
    }

    /// Waits until task `key` has ended `runs` runs and is in review; fails at once, naming the
    /// status, when it ended in another final status.
    pub async fn wait_for_runs(&self, key: &str, runs: i64) -> Result<()> {
        eventually(
            &format!("task {key} to have {runs} finished run(s)"),
            crate::wait::RUN_TIMEOUT,
            || async {
                // The count first: the status read then follows the runs it counted.
                let ended = db::completed_runs(&self.pool, key).await?;
                let in_review = crate::wait::status_reached(
                    db::task_status(&self.pool, key).await?,
                    df_core::TaskStatus::InReview,
                )?;
                Ok((in_review.is_some() && ended >= runs).then_some(()))
            },
        )
        .await
    }

    /// The client of the bot for repository `repository` of the owner.
    pub fn forgejo_for(&self, repository: &str) -> Result<Forgejo> {
        Ok(
            Forgejo::new(&self.env.forgejo_url, &self.env.forgejo_token)?
                .for_repository(repository),
        )
    }

    /// The directory of this test's diagnostics.
    pub fn artifacts_dir(&self) -> PathBuf {
        artifact_dir(&artifacts_base(), &self.name)
    }
}

/// Runs `body` as test `name` with a [`World`] and tears down afterwards, also when the body fails
/// or panics. A panic of the body is resumed after the teardown, so `cargo test` reports it as it
/// is. The test fails when the teardown cannot cancel a task, and names it.
///
/// ```ignore
/// #[tokio::test]
/// #[ignore = "needs the cluster of `just e2e-local`"]
/// async fn a_task_completes() -> anyhow::Result<()> {
///     fixture::run("a_task_completes", |world| async move {
///         let key = world.session.create_task(&world.env, "DF", "Do it", None).await?;
///         world.wait_for_status(&key, TaskStatus::Waiting).await
///     })
///     .await
/// }
/// ```
pub async fn run<F, Fut>(name: &str, body: F) -> Result<()>
where
    F: FnOnce(Arc<World>) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let world = Arc::new(Box::pin(World::start(name)).await?);
    let outcome = Outcome::from(tokio::spawn(body(Arc::clone(&world))).await);
    let teardown = teardown(&world, !matches!(outcome, Outcome::Passed)).await;
    finish(outcome, teardown)
}

/// How the body ended.
enum Outcome {
    Passed,
    Failed(anyhow::Error),
    Panicked(Box<dyn Any + Send>),
}

impl From<Result<Result<()>, JoinError>> for Outcome {
    fn from(joined: Result<Result<()>, JoinError>) -> Self {
        match joined {
            Ok(Ok(())) => Self::Passed,
            Ok(Err(e)) => Self::Failed(e),
            Err(e) => match e.try_into_panic() {
                Ok(payload) => Self::Panicked(payload),
                Err(e) => Self::Failed(anyhow!("the test body was cancelled: {e}")),
            },
        }
    }
}

/// What the teardown found.
#[derive(Debug, Default)]
struct Teardown {
    /// Tasks that neither the UI nor the fallback could cancel.
    leaked: Vec<String>,
    /// Lookups of the test's tasks that failed: tasks the teardown could not see may still run.
    lookup_failures: Vec<String>,
    /// Steps that failed but left the world clean (a cancellation through the UI that the
    /// fallback took over, a screenshot that could not be taken).
    notes: Vec<String>,
}

/// The result of the test: the body's failure or panic wins over the teardown's, a clean body
/// fails on a leaked task.
fn finish(outcome: Outcome, teardown: Teardown) -> Result<()> {
    report_teardown(&teardown.notes);
    let mut problems = Vec::new();
    if !teardown.leaked.is_empty() {
        problems.push(format!(
            "tasks left running: {}",
            teardown.leaked.join(", ")
        ));
    }
    if !teardown.lookup_failures.is_empty() {
        problems.push(format!(
            "tasks may be left running, their lookup failed: {}",
            teardown.lookup_failures.join("; ")
        ));
    }
    let leaked = (!problems.is_empty()).then(|| problems.join("; "));
    match outcome {
        Outcome::Panicked(payload) => {
            if let Some(leaked) = leaked {
                report_teardown(&[leaked]);
            }
            std::panic::resume_unwind(payload)
        }
        Outcome::Failed(e) => Err(match leaked {
            Some(leaked) => e.context(leaked),
            None => e,
        }),
        Outcome::Passed => leaked.map_or(Ok(()), |leaked| Err(anyhow!(leaked))),
    }
}

fn report_teardown(lines: &[String]) {
    for line in lines {
        report("teardown", line);
    }
}

async fn teardown(world: &World, failed: bool) -> Teardown {
    let mut teardown = Teardown::default();
    if failed {
        let dir = world.artifacts_dir();
        let failures = world.session.save_diagnostics(&dir).await;
        teardown
            .notes
            .extend(failures.into_iter().map(|f| format!("diagnostics: {f}")));
        teardown
            .notes
            .push(format!("diagnostics written to {}", dir.display()));
    }
    // After the diagnostics, which need the page: its event streams would hold the connections of
    // the fresh pages that cancel.
    world.session.close_page().await;
    cancel_tasks(world, &mut teardown).await;
    teardown
}

/// Cancels the tasks the session created and, repeatedly, the tasks they created (found through
/// `parent_id`): a coordinator may create a child until its run has stopped, so the query repeats
/// after each cancellation round until no new task appears. A failed lookup fails the test; the
/// tasks already known are still cancelled, then the teardown stops.
async fn cancel_tasks(world: &World, teardown: &mut Teardown) {
    let mut handled = BTreeSet::new();
    loop {
        let roots = root_tasks(world, &mut teardown.lookup_failures).await;
        let tasks = match with_descendants(&world.pool, roots.clone()).await {
            Ok(tasks) => tasks,
            Err(e) => {
                teardown
                    .lookup_failures
                    .push(format!("find the child tasks: {e:#}"));
                roots
            }
        };
        let complete = teardown.lookup_failures.is_empty();
        let fresh: Vec<String> = tasks
            .into_iter()
            .filter(|key| !handled.contains(key))
            .collect();
        if fresh.is_empty() {
            return;
        }
        for key in fresh {
            // A chat can't be cancelled and its runs end with their turn (`keepAliveSeconds: 0`):
            // it is only looked at for the tasks it created.
            let cancelled = if df_core::is_chat_key(&key) {
                Ok(())
            } else {
                cancel(world, &key, &mut teardown.notes).await
            };
            if let Err(e) = cancelled {
                teardown.notes.push(format!("cancel {key}: {e:#}"));
                teardown.leaked.push(key.clone());
            } else if let Err(e) = wait_for_runs_to_end(&world.pool, &key).await {
                // A run that goes on may still create a child the next lookup misses.
                teardown.notes.push(format!("{e:#}"));
                teardown.leaked.push(key.clone());
            }
            handled.insert(key);
        }
        if !complete {
            return;
        }
    }
}

/// The tasks the session created, and the ones a creation may have left although its page never
/// opened (found by project and the suffixed description, created since the submit).
async fn root_tasks(world: &World, failures: &mut Vec<String>) -> Vec<String> {
    let mut roots = world.session.created_tasks();
    for task in world.session.unlanded_tasks() {
        let found = db::tasks_submitted_since(
            &world.pool,
            &task.project,
            &task.description,
            task.submitted_at,
        )
        .await;
        match found {
            Ok(found) => roots.extend(found),
            Err(e) => failures.push(format!(
                "find the task `{}` of {}: {e:#}",
                task.description, task.project
            )),
        }
    }
    roots
}

/// `keys` and all tasks created by them, directly or not, each once.
async fn with_descendants(pool: &PgPool, keys: Vec<String>) -> Result<Vec<String>> {
    let mut all = Vec::new();
    let mut queue = keys;
    while let Some(key) = queue.pop() {
        if all.contains(&key) {
            continue;
        }
        queue.extend(db::children(pool, &key).await?);
        all.push(key);
    }
    all.sort();
    Ok(all)
}

/// Cancels task `key` unless it is final: through the UI, and over a new connection when the UI
/// does not manage. Fails when the task is not final afterwards.
async fn cancel(world: &World, key: &str, notes: &mut Vec<String>) -> Result<()> {
    if is_final(&world.pool, key).await? {
        return Ok(());
    }
    let through_ui = world.session.cancel_task_in_new_page(&world.env, key).await;
    if let Err(e) = &through_ui {
        notes.push(format!("{key}: cancel through the UI: {e:#}"));
    }
    if became_final(&world.pool, key).await {
        return Ok(());
    }
    if through_ui.is_ok() {
        notes.push(format!("{key}: the UI cancellation did not take effect"));
    }
    let cookie = world
        .session
        .cookie_header()
        .context("no session cookie: the login did not finish")?;
    cancel_task_over_new_connection(&world.env, &cookie, key)
        .await
        .context("the fallback cancellation")?;
    if became_final(&world.pool, key).await {
        return Ok(());
    }
    bail!(
        "the task is still `{}`",
        db::task_status(&world.pool, key).await?
    )
}

async fn is_final(pool: &PgPool, key: &str) -> Result<bool> {
    Ok(db::task_status(pool, key).await?.group() == StatusGroup::Done)
}

/// Waits briefly for the task to be final: the cancellation is applied by the control plane after
/// the answer to the request.
async fn became_final(pool: &PgPool, key: &str) -> bool {
    eventually(&format!("task {key} to be final"), UI_TIMEOUT, || async {
        Ok(is_final(pool, key).await?.then_some(()))
    })
    .await
    .is_ok()
}

async fn wait_for_runs_to_end(pool: &PgPool, key: &str) -> Result<()> {
    eventually(
        &format!("the runs of {key} to end"),
        RUNS_END_TIMEOUT,
        || async {
            Ok(db::runs(pool, key)
                .await?
                .iter()
                .all(|run| run.outcome.is_some())
                .then_some(()))
        },
    )
    .await
}

/// First 8 hex characters of a random UUID.
fn unique_suffix() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

fn artifacts_base() -> PathBuf {
    std::env::var_os("E2E_ARTIFACTS_DIR")
        .map_or_else(|| PathBuf::from(DEFAULT_ARTIFACTS_DIR), PathBuf::from)
}

/// `<base>/<test name>`; characters a path should not hold become `_`.
fn artifact_dir(base: &Path, test: &str) -> PathBuf {
    let name: String = test
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    base.join(name)
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;

    use super::*;

    #[test]
    fn artifacts_of_a_test_go_into_a_directory_of_its_name() {
        let dir = artifact_dir(Path::new("target/e2e-artifacts"), "a_task_completes");

        assert_eq!(dir, Path::new("target/e2e-artifacts/a_task_completes"));
    }

    #[test]
    fn a_test_name_cannot_leave_the_artifacts_directory() {
        let dir = artifact_dir(Path::new("/artifacts"), "../../etc/passwd");

        assert_eq!(dir, Path::new("/artifacts/______etc_passwd"));
    }

    #[test]
    fn the_suffix_is_eight_hex_characters_and_differs_between_calls() {
        let first = unique_suffix();
        let second = unique_suffix();

        assert_eq!(first.len(), 8, "{first}");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
        assert_ne!(first, second);
    }

    #[test]
    fn a_passing_body_with_a_clean_teardown_passes() {
        finish(Outcome::Passed, Teardown::default()).unwrap();
    }

    #[test]
    fn a_passing_body_fails_naming_the_tasks_the_teardown_could_not_cancel() {
        let teardown = Teardown {
            leaked: vec!["DF-3".into(), "DG-1".into()],
            ..Teardown::default()
        };

        let error = finish(Outcome::Passed, teardown).expect_err("leaked tasks fail the test");

        assert!(error.to_string().contains("DF-3, DG-1"), "{error}");
    }

    #[test]
    fn a_passing_body_fails_when_the_teardown_could_not_look_up_its_tasks() {
        let teardown = Teardown {
            lookup_failures: vec!["find the child tasks of DF-3: connection reset".into()],
            ..Teardown::default()
        };

        let error = finish(Outcome::Passed, teardown).expect_err("a failed lookup fails the test");

        assert!(error.to_string().contains("connection reset"), "{error}");
    }

    #[test]
    fn a_failing_body_keeps_its_error_and_names_leaked_tasks() {
        let teardown = Teardown {
            leaked: vec!["DF-3".into()],
            ..Teardown::default()
        };

        let error = finish(Outcome::Failed(anyhow!("assertion failed")), teardown)
            .expect_err("the body failed");

        assert_eq!(
            format!("{error:#}"),
            "tasks left running: DF-3: assertion failed"
        );
    }

    #[tokio::test]
    async fn a_panic_of_the_body_is_resumed_after_the_teardown() {
        let body = async {
            let result: Result<()> = Err(anyhow!("never reached"));
            assert!(result.is_ok(), "the assertion of the body");
            result
        };
        let outcome = Outcome::from(tokio::spawn(body).await);

        let panic =
            std::panic::catch_unwind(AssertUnwindSafe(|| finish(outcome, Teardown::default())))
                .expect_err("finish resumes the panic");

        let message = panic.downcast_ref::<&str>().copied().unwrap_or_default();
        assert_eq!(message, "the assertion of the body");
    }
}
