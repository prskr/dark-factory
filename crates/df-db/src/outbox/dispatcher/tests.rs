use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use tokio::sync::{Semaphore, mpsc};

use super::*;
use crate::outbox::enqueue;

const KIND: &str = "test.a";
const OTHER_KIND: &str = "test.b";

const POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_secs(30),
    cap: Duration::from_secs(60),
    give_up_after: Duration::from_hours(24),
    lease: Duration::from_secs(15),
};

/// A handler that answers from a script (then `Ok`), reports every payload and optionally waits
/// for a permit of `gate` before it answers.
struct Fake {
    kind: &'static str,
    script: Mutex<VecDeque<Result<(), JobError>>>,
    always_retry: bool,
    seen: mpsc::UnboundedSender<Value>,
    gate: Option<Arc<Semaphore>>,
    running: AtomicUsize,
    max_running: AtomicUsize,
}

impl Fake {
    fn new(kind: &'static str) -> (Self, mpsc::UnboundedReceiver<Value>) {
        let (seen, rx) = mpsc::unbounded_channel();
        let fake = Self {
            kind,
            script: Mutex::default(),
            always_retry: false,
            seen,
            gate: None,
            running: AtomicUsize::new(0),
            max_running: AtomicUsize::new(0),
        };
        (fake, rx)
    }

    fn answering(self, answers: impl IntoIterator<Item = Result<(), JobError>>) -> Self {
        self.script.lock().unwrap().extend(answers);
        self
    }
}

#[async_trait]
impl OutboxHandler for Fake {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn retry_policy(&self) -> RetryPolicy {
        POLICY
    }

    async fn handle(&self, payload: &Value) -> Result<(), JobError> {
        let running = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_running.fetch_max(running, Ordering::SeqCst);
        self.seen.send(payload.clone()).unwrap();
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        self.running.fetch_sub(1, Ordering::SeqCst);
        if self.always_retry {
            return Err(JobError::Retry("still down".into()));
        }
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(()))
    }
}

async fn setup() -> (PgPool, Arc<Notifier>) {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let notifier = Arc::new(Notifier::start(&pool).await.unwrap());
    (pool, notifier)
}

fn dispatcher(
    pool: &PgPool,
    notifier: &Arc<Notifier>,
    handlers: Vec<Arc<dyn OutboxHandler>>,
) -> Dispatcher {
    Dispatcher::new(
        pool.clone(),
        notifier.clone(),
        handlers,
        &opentelemetry::global::meter("test"),
    )
}

async fn enqueue_job(pool: &PgPool, kind: &str, payload: Value) -> i64 {
    let mut tx = pool.begin().await.unwrap();
    enqueue(&mut tx, kind, &payload).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query_scalar("SELECT max(id) FROM outbox")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[derive(Debug, sqlx::FromRow)]
struct Row {
    attempts: i32,
    done: bool,
    gave_up: bool,
    leased: bool,
    last_error: Option<String>,
    /// Seconds from now until `next_attempt_at`.
    due_in: f64,
}

async fn row(pool: &PgPool, id: i64) -> Row {
    sqlx::query_as(
        "SELECT attempts, done_at IS NOT NULL AS done, gave_up_at IS NOT NULL AS gave_up,
                lease_until IS NOT NULL AS leased, last_error,
                EXTRACT(EPOCH FROM next_attempt_at - now())::float8 AS due_in
         FROM outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn make_due(pool: &PgPool, id: i64) {
    sqlx::query("UPDATE outbox SET next_attempt_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn backoff_doubles_up_to_the_cap() {
    let policy = RetryPolicy {
        base: Duration::from_secs(30),
        cap: Duration::from_hours(1),
        ..POLICY
    };
    let delays: Vec<u64> = (1..=9).map(|n| policy.backoff(n).as_secs()).collect();
    assert_eq!(delays, [30, 60, 120, 240, 480, 960, 1920, 3600, 3600]);
    assert_eq!(policy.backoff(u32::MAX), Duration::from_hours(1));
}

#[tokio::test]
async fn ok_marks_the_job_done() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let id = enqueue_job(&pool, KIND, json!({"n": 1})).await;

    assert_eq!(dispatcher.run_due().await.unwrap(), 1);

    assert_eq!(seen.recv().await.unwrap(), json!({"n": 1}));
    let row = row(&pool, id).await;
    assert!(row.done && !row.gave_up && !row.leased, "{row:?}");
    assert_eq!(row.attempts, 1);
    assert_eq!(
        dispatcher.run_due().await.unwrap(),
        0,
        "a done job is not claimed again"
    );
}

#[tokio::test]
async fn retry_follows_the_handlers_policy() {
    let (pool, notifier) = setup().await;
    let (fake, _seen) = Fake::new(KIND);
    let fake = fake.answering([
        Err(JobError::Retry("503".into())),
        Err(JobError::Retry("503".into())),
        Err(JobError::Retry("503".into())),
    ]);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let id = enqueue_job(&pool, KIND, json!({})).await;

    for (attempt, delay) in [(1, 30.0), (2, 60.0), (3, 60.0)] {
        assert_eq!(dispatcher.run_due().await.unwrap(), 1);
        let row = row(&pool, id).await;
        assert_eq!(row.attempts, attempt);
        assert!(!row.done && !row.gave_up && !row.leased, "{row:?}");
        assert_eq!(row.last_error.as_deref(), Some("503"));
        assert!(
            (row.due_in - delay).abs() < 5.0,
            "attempt {attempt}: {row:?}"
        );
        assert_eq!(dispatcher.run_due().await.unwrap(), 0, "not due yet");
        make_due(&pool, id).await;
    }
}

#[tokio::test]
async fn a_give_up_of_the_handler_gives_the_job_up() {
    let (pool, notifier) = setup().await;
    let (fake, _seen) = Fake::new(KIND);
    let fake = fake.answering([Err(JobError::GiveUp("gone".into()))]);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let id = enqueue_job(&pool, KIND, json!({})).await;

    assert_eq!(dispatcher.run_due().await.unwrap(), 1);

    let row = row(&pool, id).await;
    assert!(row.gave_up && !row.done && !row.leased, "{row:?}");
    assert_eq!(row.last_error.as_deref(), Some("gone"));
    make_due(&pool, id).await;
    assert_eq!(dispatcher.run_due().await.unwrap(), 0, "given-up jobs stay");
}

#[tokio::test]
async fn a_job_claimed_past_the_give_up_age_is_given_up_without_running() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let failed_before = enqueue_job(&pool, KIND, json!({})).await;
    let never_ran = enqueue_job(&pool, KIND, json!({})).await;
    sqlx::query(
        "UPDATE outbox SET created_at = now() - interval '24 hours',
             last_error = CASE WHEN id = $1 THEN '503' END",
    )
    .bind(failed_before)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(dispatcher.run_due().await.unwrap(), 2);

    assert!(seen.try_recv().is_err(), "the handler ran");
    for (id, error) in [
        (failed_before, "503"),
        (never_ran, "expired before the first attempt"),
    ] {
        let row = row(&pool, id).await;
        assert!(row.gave_up && !row.done && !row.leased, "{row:?}");
        assert_eq!(row.last_error.as_deref(), Some(error));
    }
}

#[tokio::test]
async fn a_job_just_below_the_give_up_age_still_runs() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let id = enqueue_job(&pool, KIND, json!({"n": 1})).await;
    sqlx::query("UPDATE outbox SET created_at = now() - interval '23 hours 59 minutes'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(dispatcher.run_due().await.unwrap(), 1);

    assert_eq!(seen.recv().await.unwrap(), json!({"n": 1}));
    assert!(row(&pool, id).await.done);
}

#[tokio::test]
async fn a_claim_takes_only_as_many_jobs_as_run_at_once() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let gate = Arc::new(Semaphore::new(0));
    let fake = Arc::new(Fake {
        gate: Some(gate.clone()),
        ..fake
    });
    let dispatcher = Arc::new(dispatcher(&pool, &notifier, vec![fake.clone()]));
    let mut ids = Vec::new();
    for n in 0..=MAX_IN_FLIGHT {
        ids.push(enqueue_job(&pool, KIND, json!({"n": n})).await);
    }

    let run = tokio::spawn({
        let dispatcher = dispatcher.clone();
        async move { dispatcher.run_due().await.unwrap() }
    });
    for _ in 0..MAX_IN_FLIGHT {
        seen.recv().await.unwrap();
    }
    assert_eq!(fake.running.load(Ordering::SeqCst), MAX_IN_FLIGHT);
    let last = row(&pool, *ids.last().unwrap()).await;
    assert!(last.attempts == 0 && !last.leased, "{last:?}");
    gate.add_permits(MAX_IN_FLIGHT + 1);

    assert_eq!(run.await.unwrap(), MAX_IN_FLIGHT);
    assert_eq!(dispatcher.run_due().await.unwrap(), 1);
    assert_eq!(fake.max_running.load(Ordering::SeqCst), MAX_IN_FLIGHT);
}

#[tokio::test]
async fn an_expired_lease_is_retried_after_a_crash() {
    let (pool, notifier) = setup().await;
    let (fake, _seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let crashed = enqueue_job(&pool, KIND, json!({})).await;
    let running = enqueue_job(&pool, KIND, json!({})).await;
    sqlx::query(
        "UPDATE outbox SET attempts = 1,
             lease_until = CASE WHEN id = $1 THEN now() - interval '1 second'
                                ELSE now() + interval '1 minute' END",
    )
    .bind(crashed)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(dispatcher.run_due().await.unwrap(), 1);

    let row_crashed = row(&pool, crashed).await;
    assert!(row_crashed.done, "{row_crashed:?}");
    assert_eq!(row_crashed.attempts, 2);
    let row_running = row(&pool, running).await;
    assert!(!row_running.done && row_running.leased, "{row_running:?}");
}

#[tokio::test]
async fn a_job_of_an_unknown_kind_is_left_untouched() {
    let (pool, notifier) = setup().await;
    let (fake, _seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let id = enqueue_job(&pool, "test.unknown", json!({})).await;

    assert_eq!(dispatcher.run_due().await.unwrap(), 0);
    dispatcher.cleanup().await.unwrap();

    let row = row(&pool, id).await;
    assert_eq!(row.attempts, 0);
    assert!(!row.done && !row.gave_up && !row.leased, "{row:?}");
}

#[tokio::test]
async fn one_failing_kind_does_not_block_another() {
    let (pool, notifier) = setup().await;
    let (failing, _seen_failing) = Fake::new(KIND);
    let failing = Fake {
        always_retry: true,
        ..failing
    };
    let (healthy, mut seen_healthy) = Fake::new(OTHER_KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(failing), Arc::new(healthy)]);
    for _ in 0..2 * MAX_IN_FLIGHT {
        enqueue_job(&pool, KIND, json!({})).await;
    }
    let healthy_job = enqueue_job(&pool, OTHER_KIND, json!({"healthy": true})).await;

    dispatcher.drain(|| false).await;

    assert_eq!(seen_healthy.recv().await.unwrap(), json!({"healthy": true}));
    assert!(row(&pool, healthy_job).await.done);
}

#[tokio::test]
async fn cleanup_removes_jobs_finished_more_than_seven_days_ago() {
    let (pool, notifier) = setup().await;
    let (fake, _seen) = Fake::new(KIND);
    let dispatcher = dispatcher(&pool, &notifier, vec![Arc::new(fake)]);
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(enqueue_job(&pool, KIND, json!({})).await);
    }
    let [old_done, old_given_up, recent_done, pending, old_pending] = ids[..] else {
        panic!("five jobs");
    };
    sqlx::query(
        "UPDATE outbox SET
             done_at = CASE WHEN id = $1 THEN now() - interval '8 days'
                            WHEN id = $3 THEN now() - interval '6 days' END,
             gave_up_at = CASE WHEN id = $2 THEN now() - interval '8 days' END,
             created_at = CASE WHEN id = $4 THEN created_at ELSE now() - interval '9 days' END",
    )
    .bind(old_done)
    .bind(old_given_up)
    .bind(recent_done)
    .bind(pending)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(dispatcher.cleanup().await.unwrap(), 2);

    let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM outbox ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, [recent_done, pending, old_pending]);
}

#[tokio::test]
async fn a_job_whose_backoff_elapsed_runs_at_the_next_configured_poll() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let fake = fake.answering([Err(JobError::Retry("down".into()))]);
    let dispatcher = Arc::new(
        dispatcher(&pool, &notifier, vec![Arc::new(fake)])
            .with_poll_interval(Duration::from_millis(100)),
    );
    let (_leader_tx, leader_rx) = watch::channel(true);
    let (stop_tx, stop_rx) = watch::channel(false);
    let run = tokio::spawn({
        let dispatcher = dispatcher.clone();
        async move { dispatcher.run(leader_rx, stop_rx).await }
    });
    let job = enqueue_job(&pool, KIND, json!({"n": 1})).await;
    let first = tokio::time::timeout(Duration::from_secs(10), seen.recv()).await;
    assert!(first.is_ok(), "the job did not run within 10 s");
    // The failed attempt records its backoff.
    let backoff = tokio::time::timeout(Duration::from_secs(10), async {
        while row(&pool, job).await.due_in < 20.0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        backoff.is_ok(),
        "the failed attempt recorded no backoff within 10 s"
    );

    // Nothing notifies when a backoff elapses: only the poll finds the job. The default poll of
    // 5 s would not get there in time.
    sqlx::query(
        "UPDATE outbox SET next_attempt_at = now() + interval '200 milliseconds' WHERE id = $1",
    )
    .bind(job)
    .execute(&pool)
    .await
    .unwrap();
    let again = tokio::time::timeout(Duration::from_secs(3), seen.recv()).await;

    stop_tx.send_replace(true);
    run.await.unwrap();
    assert!(again.is_ok(), "the job did not run again within 3 s");
}

#[tokio::test]
async fn only_the_leader_dispatches() {
    let (pool, notifier) = setup().await;
    let (fake, mut seen) = Fake::new(KIND);
    let dispatcher = Arc::new(dispatcher(&pool, &notifier, vec![Arc::new(fake)]));
    let (leader_tx, leader_rx) = watch::channel(false);
    let (stop_tx, stop_rx) = watch::channel(false);
    let run = tokio::spawn({
        let dispatcher = dispatcher.clone();
        async move { dispatcher.run(leader_rx, stop_rx).await }
    });

    let job = enqueue_job(&pool, KIND, json!({"n": 1})).await;
    // The test runtime is single-threaded and a follower does no I/O before it waits for
    // leadership, so one yield parks it there; a leader would have subscribed to notifications.
    tokio::task::yield_now().await;
    assert_eq!(notifier.subscribers(), 0, "the follower started leading");
    let unclaimed = row(&pool, job).await;
    assert!(
        unclaimed.attempts == 0 && !unclaimed.leased,
        "{unclaimed:?}"
    );

    leader_tx.send_replace(true);
    assert_eq!(seen.recv().await.unwrap(), json!({"n": 1}));
    enqueue_job(&pool, KIND, json!({"n": 2})).await;
    assert_eq!(
        seen.recv().await.unwrap(),
        json!({"n": 2}),
        "woken by the notification"
    );

    leader_tx.send_replace(false);
    stop_tx.send_replace(true);
    run.await.unwrap();
}
