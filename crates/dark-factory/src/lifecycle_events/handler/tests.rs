use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use df_agent_channel::metrics::probe::Probe;
use df_db::Notifier;
use df_db::outbox::{Dispatcher, enqueue};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::lifecycle_events::signing::testing::{generate, verifies};

const DOWN: &str = "https://down.example.org/hook";
const UP: &str = "https://up.example.org/hook";
const INSTANCE: &str = "http://audit.svc/hook";

/// What the receiver answers.
#[derive(Debug, Clone, Copy)]
enum Answer {
    Status(u16),
    Refused,
}

/// One request as sent.
#[derive(Debug)]
struct Sent {
    trust: Trust,
    url: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// Answers each URL from its script, then with its fallback (default 200).
#[derive(Default)]
struct FakeReceivers {
    scripts: Mutex<HashMap<String, VecDeque<Answer>>>,
    fallback: Mutex<HashMap<String, Answer>>,
    sent: Mutex<Vec<Sent>>,
}

impl FakeReceivers {
    fn script(&self, url: &str, answers: impl IntoIterator<Item = Answer>) {
        self.scripts
            .lock()
            .unwrap()
            .entry(url.into())
            .or_default()
            .extend(answers);
    }

    fn always(&self, url: &str, answer: Answer) {
        self.fallback.lock().unwrap().insert(url.into(), answer);
    }

    fn sent_to(&self, url: &str) -> Vec<(HeaderMap, Vec<u8>, Trust)> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.url == url)
            .map(|s| (s.headers.clone(), s.body.clone(), s.trust))
            .collect()
    }
}

#[async_trait]
impl Post for FakeReceivers {
    async fn post(
        &self,
        trust: Trust,
        url: &Url,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<StatusCode, PostError> {
        let url = url.as_str().to_owned();
        let scripted = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&url)
            .and_then(VecDeque::pop_front);
        let answer = scripted
            .or_else(|| self.fallback.lock().unwrap().get(&url).copied())
            .unwrap_or(Answer::Status(200));
        self.sent.lock().unwrap().push(Sent {
            trust,
            url,
            headers,
            body,
        });
        match answer {
            Answer::Status(code) => Ok(StatusCode::from_u16(code).unwrap()),
            Answer::Refused => Err(PostError::Refused("10.0.0.1".into())),
        }
    }
}

struct Env {
    pool: PgPool,
    receivers: Arc<FakeReceivers>,
    key: Arc<SigningKey>,
    probe: Probe,
    dispatcher: Dispatcher,
    project: Uuid,
}

impl Env {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let notifier = Arc::new(Notifier::start(&pool).await.unwrap());
        let receivers = Arc::new(FakeReceivers::default());
        let key = Arc::new(generate());
        let probe = Probe::new();
        let handler =
            DeliverHandler::new(pool.clone(), key.clone(), receivers.clone(), &probe.meter());
        let dispatcher = Dispatcher::new(
            pool.clone(),
            notifier,
            vec![Arc::new(handler)],
            &probe.meter(),
        );
        let project = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'DF', 'DF', 'r', 'i')",
        )
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
        Self {
            pool,
            receivers,
            key,
            probe,
            dispatcher,
            project,
        }
    }

    async fn subscribe(&self, url: &str) -> Uuid {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO event_subscriptions (id, project_id, url, events) VALUES ($1, $2, $3, '{}')",
        )
        .bind(id)
        .bind(self.project)
        .bind(url)
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    /// Records an event that happened `age` ago and one delivery job per target.
    async fn event(&self, age: Duration, targets: &[Target]) -> Uuid {
        let id = Uuid::now_v7();
        let occurred_at = Utc::now() - age;
        let payload = json!({
            "id": format!("evt_{id}"),
            "type": "sandbox.provisioned",
            "timestamp": occurred_at.to_rfc3339(),
            "data": {},
        });
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO lifecycle_events (id, type, occurred_at, project_id, payload)
             VALUES ($1, 'sandbox.provisioned', $2, $3, $4)",
        )
        .bind(id)
        .bind(occurred_at)
        .bind(self.project)
        .bind(&payload)
        .execute(&mut *tx)
        .await
        .unwrap();
        for target in targets {
            let job = DeliverJob {
                event_id: id,
                target: target.clone(),
            };
            enqueue(&mut tx, DELIVER_KIND, &serde_json::to_value(job).unwrap())
                .await
                .unwrap();
        }
        sqlx::query("UPDATE outbox SET created_at = $1 WHERE payload->>'event_id' = $2")
            .bind(occurred_at)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn make_due(&self) {
        sqlx::query("UPDATE outbox SET next_attempt_at = now()")
            .execute(&self.pool)
            .await
            .unwrap();
    }

    /// `(attempts, done, gave_up, seconds until the next attempt)` of the job to `target`.
    async fn job(&self, target: &Target) -> (i32, bool, bool, f64) {
        sqlx::query_as(
            "SELECT attempts, done_at IS NOT NULL, gave_up_at IS NOT NULL,
                    EXTRACT(EPOCH FROM next_attempt_at - now())::float8
             FROM outbox WHERE payload->'target' = $1",
        )
        .bind(serde_json::to_value(target).unwrap())
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn subscription_status(&self, id: Uuid) -> (bool, Option<i16>, Option<String>) {
        sqlx::query_as(
            "SELECT last_attempt_at IS NOT NULL, last_status, last_error
             FROM event_subscriptions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    fn deliveries(&self, result: &str) -> f64 {
        self.probe
            .total("df.lifecycle_event.deliveries", &[("result", result)])
    }

    fn outbox_jobs(&self, result: &str) -> f64 {
        self.probe.total(
            "df.outbox.jobs",
            &[("kind", DELIVER_KIND), ("result", result)],
        )
    }

    async fn last_error(&self, target: &Target) -> Option<String> {
        sqlx::query_scalar("SELECT last_error FROM outbox WHERE payload->'target' = $1")
            .bind(serde_json::to_value(target).unwrap())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

fn subscription(subscription_id: Uuid) -> Target {
    Target::Subscription { subscription_id }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).unwrap().to_str().unwrap()
}

#[test]
fn backoff_is_30_s_doubling_up_to_an_hour_and_given_up_after_a_day() {
    let delays: Vec<u64> = (1..=9).map(|n| RETRY_POLICY.backoff(n).as_secs()).collect();
    assert_eq!(delays, [30, 60, 120, 240, 480, 960, 1920, 3600, 3600]);
    assert_eq!(RETRY_POLICY.give_up_after, Duration::from_hours(24));
}

#[tokio::test]
async fn a_2xx_delivers_a_signed_event_and_records_the_status() {
    let env = Env::new().await;
    let sub = env.subscribe(UP).await;
    let event_id = env.event(Duration::ZERO, &[subscription(sub)]).await;

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    let sent = env.receivers.sent_to(UP);
    let [(headers, body, trust)] = &sent[..] else {
        panic!("one request: {sent:?}");
    };
    assert_eq!(*trust, Trust::Project);
    let webhook_id = header(headers, "webhook-id");
    assert_eq!(webhook_id, format!("evt_{event_id}"));
    let timestamp = header(headers, "webhook-timestamp");
    let message = [format!("{webhook_id}.{timestamp}.").as_bytes(), body].concat();
    assert!(verifies(
        &env.key,
        header(headers, "webhook-signature"),
        &message
    ));
    let payload: Value = serde_json::from_slice(body).unwrap();
    assert_eq!(payload["id"], json!(webhook_id));
    assert_eq!(env.subscription_status(sub).await, (true, Some(200), None));
    assert!(env.job(&subscription(sub)).await.1, "done");
    assert!((env.deliveries("delivered") - 1.0).abs() < f64::EPSILON);
    assert!((env.outbox_jobs("done") - 1.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn a_receiver_down_twice_gets_the_event_on_the_third_attempt() {
    let env = Env::new().await;
    let sub = env.subscribe(UP).await;
    env.receivers
        .script(UP, [Answer::Status(503), Answer::Status(503)]);
    // An hour-old event: a fresh `webhook-timestamp` cannot be the event time.
    env.event(Duration::from_hours(1), &[subscription(sub)])
        .await;

    for (attempt, delay) in [(1, 30.0), (2, 60.0)] {
        assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);
        let (attempts, done, gave_up, due_in) = env.job(&subscription(sub)).await;
        assert_eq!((attempts, done, gave_up), (attempt, false, false));
        assert!((due_in - delay).abs() < 5.0, "attempt {attempt}: {due_in}");
        assert_eq!(env.subscription_status(sub).await, (true, Some(503), None));
        env.make_due().await;
    }
    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    assert!(env.job(&subscription(sub)).await.1, "delivered");
    let sent = env.receivers.sent_to(UP);
    assert_eq!(sent.len(), 3);
    let now = Utc::now().timestamp();
    for (headers, body, _) in &sent {
        assert_eq!(
            header(headers, "webhook-id"),
            header(&sent[0].0, "webhook-id")
        );
        let timestamp: i64 = header(headers, "webhook-timestamp").parse().unwrap();
        assert!(
            (now - timestamp).abs() < 60,
            "{timestamp} is not the attempt time"
        );
        let message = [
            format!("{}.{timestamp}.", header(headers, "webhook-id")).as_bytes(),
            body,
        ]
        .concat();
        assert!(verifies(
            &env.key,
            header(headers, "webhook-signature"),
            &message
        ));
    }
    assert!((env.deliveries("failed") - 2.0).abs() < f64::EPSILON);
    assert!((env.deliveries("delivered") - 1.0).abs() < f64::EPSILON);
    assert!((env.outbox_jobs("retry") - 2.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn a_job_due_a_day_after_the_event_is_given_up_without_sending() {
    let env = Env::new().await;
    let sub = env.subscribe(DOWN).await;
    env.event(Duration::from_hours(25), &[subscription(sub)])
        .await;

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    assert!(env.receivers.sent_to(DOWN).is_empty());
    let (attempts, done, gave_up, _) = env.job(&subscription(sub)).await;
    assert_eq!((attempts, done, gave_up), (1, false, true));
    assert!(env.deliveries("failed").abs() < f64::EPSILON);
    assert!((env.outbox_jobs("gave_up") - 1.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn a_deleted_subscription_gives_up_its_jobs() {
    let env = Env::new().await;
    let sub = env.subscribe(UP).await;
    env.event(Duration::ZERO, &[subscription(sub)]).await;
    sqlx::query("DELETE FROM event_subscriptions WHERE id = $1")
        .bind(sub)
        .execute(&env.pool)
        .await
        .unwrap();

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    assert!(env.receivers.sent_to(UP).is_empty());
    assert!(env.job(&subscription(sub)).await.2, "given up");
    assert!((env.outbox_jobs("gave_up") - 1.0).abs() < f64::EPSILON);
}

#[tokio::test]
async fn one_dead_receiver_does_not_block_another() {
    let env = Env::new().await;
    let down = env.subscribe(DOWN).await;
    let up = env.subscribe(UP).await;
    env.receivers.always(DOWN, Answer::Status(500));
    let instance = Target::Instance {
        instance_url: INSTANCE.into(),
    };
    env.event(
        Duration::ZERO,
        &[subscription(down), subscription(up), instance.clone()],
    )
    .await;

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 3);

    assert!(!env.job(&subscription(down)).await.1);
    assert!(env.job(&subscription(up)).await.1);
    assert!(env.job(&instance).await.1);
    let [(_, _, trust)] = &env.receivers.sent_to(INSTANCE)[..] else {
        panic!("one request to the instance subscription");
    };
    assert_eq!(*trust, Trust::Instance);
}

#[tokio::test]
async fn a_refused_address_is_a_failed_attempt_naming_the_reason() {
    let env = Env::new().await;
    let sub = env.subscribe(DOWN).await;
    env.receivers.always(DOWN, Answer::Refused);
    env.event(Duration::ZERO, &[subscription(sub)]).await;

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    let (ok, status, error) = env.subscription_status(sub).await;
    assert!(ok);
    assert_eq!(status, None);
    assert!(error.unwrap().contains("address refused"));
    let (_, done, gave_up, _) = env.job(&subscription(sub)).await;
    assert!(!done && !gave_up, "retried");
    assert!((env.deliveries("refused") - 1.0).abs() < f64::EPSILON);
    let job_error = env.last_error(&subscription(sub)).await.unwrap();
    assert!(
        job_error.starts_with(&format!("subscription {sub}: address refused")),
        "{job_error}"
    );
}

#[tokio::test]
async fn a_failed_instance_attempt_names_only_the_receiver_host() {
    let env = Env::new().await;
    let url = "https://user:secret@audit.example.org:8443/hook/token?key=secret";
    env.receivers.always(url, Answer::Status(500));
    let instance = Target::Instance {
        instance_url: url.into(),
    };
    env.event(Duration::ZERO, std::slice::from_ref(&instance))
        .await;

    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);

    assert_eq!(
        env.last_error(&instance).await.as_deref(),
        Some("instance receiver audit.example.org:8443: HTTP 500 Internal Server Error")
    );
}

#[tokio::test]
async fn cleanup_removes_events_without_jobs() {
    let env = Env::new().await;
    let sub = env.subscribe(UP).await;
    env.event(Duration::ZERO, &[subscription(sub)]).await;
    assert_eq!(env.dispatcher.run_due().await.unwrap(), 1);
    sqlx::query("UPDATE outbox SET done_at = now() - interval '8 days'")
        .execute(&env.pool)
        .await
        .unwrap();

    assert_eq!(env.dispatcher.cleanup().await.unwrap(), 1);

    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM lifecycle_events")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
}

#[test]
fn a_refusal_is_described_with_the_refused_host() {
    let refused = PostError::Refused("10.0.0.1".into());
    assert_eq!(
        describe(&refused),
        "address refused: 10.0.0.1 is not a public address"
    );
}
