//! Tests of the `pr.sync` handler against the fake forge.

use chrono::TimeDelta;
use df_core::testing::ForgeCall;
use df_core::{PullRequestRef, PullRequestState, RepoRef, UpstreamError};
use uuid::Uuid;

use super::*;
use crate::testing::Env;

struct Fixture {
    env: Env,
    run: RunId,
}

impl Fixture {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        Self { env, run }
    }

    fn handler(&self) -> PrSyncHandler {
        PrSyncHandler::new(
            self.env.ctx.pool.clone(),
            Arc::new(self.env.forge.clone()),
            Duration::from_secs(30),
        )
    }

    fn job(&self, open: bool, ready: Option<DateTime<Utc>>) -> PrSyncJob {
        PrSyncJob {
            task_id: self.env.task.id,
            run_id: Some(self.run),
            open,
            ready,
        }
    }

    async fn run(&self, job: PrSyncJob) -> Result<(), JobError> {
        self.handler()
            .handle(&serde_json::to_value(job).unwrap())
            .await
    }

    async fn set_status(&self, status: &str) {
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(self.env.task.id.as_uuid())
            .bind(status)
            .execute(&self.env.ctx.pool)
            .await
            .unwrap();
    }

    async fn set_summary(&self, run: RunId, summary: &str) {
        sqlx::query("UPDATE runs SET completion_summary = $2 WHERE id = $1")
            .bind(run.as_uuid())
            .bind(summary)
            .execute(&self.env.ctx.pool)
            .await
            .unwrap();
    }

    async fn pr_number(&self) -> Option<i64> {
        sqlx::query_scalar("SELECT pr_number FROM tasks WHERE id = $1")
            .bind(self.env.task.id.as_uuid())
            .fetch_one(&self.env.ctx.pool)
            .await
            .unwrap()
    }

    fn creates(&self) -> Vec<NewPullRequest> {
        self.env
            .forge
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                ForgeCall::CreatePullRequest { request, .. } => Some(request),
                _ => None,
            })
            .collect()
    }

    /// `(number, body)` of the comments posted.
    fn comments(&self) -> Vec<(u64, String)> {
        self.env
            .forge
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                ForgeCall::AddIssueComment { number, body, .. } => Some((number, body)),
                _ => None,
            })
            .collect()
    }

    /// Numbers of the pull requests marked ready.
    fn ready_marks(&self) -> Vec<u64> {
        self.env
            .forge
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                ForgeCall::UpdatePullRequest {
                    number, changes, ..
                } if changes.draft == Some(false) => Some(number),
                _ => None,
            })
            .collect()
    }

    async fn forge_events(&self) -> Vec<(Uuid, String, Value)> {
        sqlx::query_as(
            "SELECT run_id, source_key, payload -> 'forgeActivity'
             FROM transcript_events WHERE kind = 'forge_activity' ORDER BY id",
        )
        .fetch_all(&self.env.ctx.pool)
        .await
        .unwrap()
    }

    async fn queue_message_at(&self, at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO task_messages (id, task_id, author, content, queued_at)
             VALUES ($1, $2, '{}', '[]', $3)",
        )
        .bind(Uuid::now_v7())
        .bind(self.env.task.id.as_uuid())
        .bind(at)
        .execute(&self.env.ctx.pool)
        .await
        .unwrap();
    }
}

fn pull_request(number: u64) -> PullRequest {
    PullRequest {
        id: PullRequestRef {
            repo: RepoRef {
                owner: "peter".into(),
                name: "dark-factory".into(),
            },
            number,
        },
        url: format!("https://git.example.org/peter/dark-factory/pulls/{number}"),
        head: "factory/task/DF-1".into(),
        base: "main".into(),
        title: "t".into(),
        body: "b".into(),
        draft: true,
        state: PullRequestState::Open,
        head_sha: String::new(),
    }
}

fn bad_gateway() -> IntegrationError {
    IntegrationError::Upstream(UpstreamError {
        status: 502,
        message: "bad gateway".into(),
    })
}

#[test]
fn a_negative_stored_pull_request_number_is_invalid_data() {
    let error = RecordedPr::from_row(-1, None, false).err().unwrap();
    assert!(matches!(error, ChannelError::InvalidData(_)));
    assert_eq!(RecordedPr::from_row(7, None, false).unwrap().number, 7);
}

#[tokio::test]
async fn open_creates_and_records_a_draft_with_default_title_and_body() {
    let f = Fixture::new().await;

    f.run(f.job(true, None)).await.unwrap();

    let created = f.creates();
    assert_eq!(created.len(), 1);
    assert!(created[0].draft);
    assert_eq!(
        (created[0].head.as_str(), created[0].base.as_str()),
        ("factory/task/DF-1", "main")
    );
    assert_eq!(created[0].title, "DF-1: Fix login");
    assert!(created[0].body.ends_with("Task: DF-1"));
    assert_eq!(f.pr_number().await, Some(1));
}

#[tokio::test]
async fn stored_title_and_body_are_used() {
    let f = Fixture::new().await;
    sqlx::query("UPDATE tasks SET pr_title = 'Better title', pr_body = 'Better body'")
        .execute(&f.env.ctx.pool)
        .await
        .unwrap();

    f.run(f.job(true, None)).await.unwrap();

    assert_eq!(f.creates()[0].title, "Better title");
    assert_eq!(f.creates()[0].body, "Better body");
}

#[tokio::test]
async fn open_writes_one_pull_request_opened_event_in_its_run() {
    let f = Fixture::new().await;

    f.run(f.job(true, None)).await.unwrap();
    f.run(f.job(true, None)).await.unwrap();

    let events = f.forge_events().await;
    assert_eq!(events.len(), 1);
    let (run, key, payload) = &events[0];
    assert_eq!(*run, f.run.as_uuid());
    assert_eq!(key, "pr-opened:1");
    assert_eq!(payload["pullRequestOpened"]["number"], "1");
    assert_eq!(payload["pullRequestOpened"]["draft"], true);
}

#[tokio::test]
async fn a_recorded_pull_request_is_not_opened_again() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    let calls = f.env.forge.calls().len();

    f.run(f.job(true, None)).await.unwrap();

    assert_eq!(f.env.forge.calls().len(), calls);
    assert_eq!(f.creates().len(), 1);
}

#[tokio::test]
async fn an_existing_open_pull_request_is_recorded_instead_of_creating_a_second() {
    let f = Fixture::new().await;
    f.env
        .forge
        .push_find_open_pull_request_result(Ok(Some(pull_request(7))));

    f.run(f.job(true, None)).await.unwrap();

    assert!(f.creates().is_empty());
    assert_eq!(f.pr_number().await, Some(7));
    assert_eq!(f.forge_events().await[0].1, "pr-opened:7");
}

#[tokio::test]
async fn a_creation_conflict_records_the_existing_pull_request() {
    let f = Fixture::new().await;
    f.env.forge.push_find_open_pull_request_result(Ok(None));
    f.env
        .forge
        .push_create_pull_request_result(Err(IntegrationError::Conflict("exists".into())));
    f.env
        .forge
        .push_find_open_pull_request_result(Ok(Some(pull_request(9))));

    f.run(f.job(true, None)).await.unwrap();

    assert_eq!(f.pr_number().await, Some(9));
}

#[tokio::test]
async fn no_pull_request_is_opened_for_a_completed_or_cancelled_task() {
    for status in ["completed", "cancelled"] {
        let f = Fixture::new().await;
        f.set_status(status).await;

        f.run(f.job(true, None)).await.unwrap();

        assert!(f.creates().is_empty(), "{status}");
        assert_eq!(f.pr_number().await, None, "{status}");
    }
}

#[tokio::test]
async fn an_existing_pr_of_a_completed_task_is_recorded_without_creating_one() {
    let f = Fixture::new().await;
    f.set_status("completed").await;
    f.env
        .forge
        .push_find_open_pull_request_result(Ok(Some(pull_request(7))));

    f.run(f.job(true, None)).await.unwrap();

    assert!(f.creates().is_empty());
    assert_eq!(f.pr_number().await, Some(7));
}

#[tokio::test]
async fn a_cancelled_task_is_not_marked_ready_and_gets_no_comment() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_status("cancelled").await;
    f.set_summary(f.run, "All done").await;

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    assert!(f.ready_marks().is_empty());
    assert!(f.comments().is_empty());
}

#[tokio::test]
async fn ready_marks_the_pr_ready_and_posts_the_summary_of_the_run() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "All done").await;

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
    assert_eq!(f.comments(), [(1, "All done".to_string())]);
}

#[tokio::test]
async fn open_and_ready_in_one_job_opens_then_marks_ready_with_both_events() {
    let f = Fixture::new().await;

    f.run(f.job(true, Some(Utc::now()))).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
    let events = f.forge_events().await;
    let keys: Vec<&str> = events.iter().map(|e| e.1.as_str()).collect();
    assert_eq!(keys, ["pr-opened:1", "pr-ready:1"]);
    let ready = &events[1].2["pullRequestReady"];
    assert_eq!(ready["number"], "1");
    assert!(ready["url"].as_str().unwrap().ends_with("/1"));
}

#[tokio::test]
async fn ready_writes_the_event_once_across_two_runs_of_the_job() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();
    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    let ready = f
        .forge_events()
        .await
        .into_iter()
        .filter(|e| e.1 == "pr-ready:1")
        .count();
    assert_eq!(ready, 1);
}

#[tokio::test]
async fn an_empty_run_summary_posts_no_comment() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "").await;

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
    assert!(f.comments().is_empty());
}

#[tokio::test]
async fn the_summary_of_an_earlier_run_is_not_posted() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "Fixed login").await;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(f.run.as_uuid())
        .execute(&f.env.ctx.pool)
        .await
        .unwrap();
    let run2 = f.env.insert_run(f.env.task.id, 2, "running").await;

    let job = PrSyncJob {
        run_id: Some(run2),
        ..f.job(false, Some(Utc::now()))
    };
    f.run(job).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
    assert!(f.comments().is_empty());
}

#[tokio::test]
async fn a_job_without_a_run_marks_ready_without_a_comment() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "Fixed login").await;

    let job = PrSyncJob {
        run_id: None,
        ..f.job(false, Some(Utc::now()))
    };
    f.run(job).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
    assert!(f.comments().is_empty());
}

#[tokio::test]
async fn a_forge_failure_on_find_create_ready_or_comment_returns_retry() {
    // find
    let f = Fixture::new().await;
    f.env
        .forge
        .push_find_open_pull_request_result(Err(bad_gateway()));
    assert!(matches!(
        f.run(f.job(true, None)).await,
        Err(JobError::Retry(_))
    ));
    // create
    let f = Fixture::new().await;
    f.env.forge.push_find_open_pull_request_result(Ok(None));
    f.env
        .forge
        .push_create_pull_request_result(Err(bad_gateway()));
    assert!(matches!(
        f.run(f.job(true, None)).await,
        Err(JobError::Retry(_))
    ));
    assert_eq!(f.pr_number().await, None);
    // ready
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.env
        .forge
        .push_update_pull_request_result(Err(bad_gateway()));
    assert!(matches!(
        f.run(f.job(false, Some(Utc::now()))).await,
        Err(JobError::Retry(_))
    ));
    assert!(
        f.forge_events()
            .await
            .iter()
            .all(|e| e.2.get("pullRequestReady").is_none())
    );
    // comment
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "All done").await;
    f.env
        .forge
        .push_add_issue_comment_result(Err(bad_gateway()));
    assert!(matches!(
        f.run(f.job(false, Some(Utc::now()))).await,
        Err(JobError::Retry(_))
    ));
    // A retry succeeds and posts the summary.
    f.run(f.job(false, Some(Utc::now()))).await.unwrap();
    assert_eq!(f.comments().len(), 2);
}

#[tokio::test]
async fn an_invalid_payload_gives_up() {
    let f = Fixture::new().await;

    let result = f.handler().handle(&serde_json::json!({})).await;

    assert!(matches!(result, Err(JobError::GiveUp(_))), "{result:?}");
}

#[tokio::test]
async fn a_ready_only_job_waits_for_an_unfinished_open_job_of_the_task() {
    let f = Fixture::new().await;
    let open = serde_json::to_value(f.job(true, None)).unwrap();
    df_db::outbox::enqueue(
        &mut f.env.ctx.pool.acquire().await.unwrap(),
        PR_SYNC_KIND,
        &open,
    )
    .await
    .unwrap();

    let result = f.run(f.job(false, Some(Utc::now()))).await;

    assert!(matches!(result, Err(JobError::Retry(_))), "{result:?}");
    assert!(f.env.forge.calls().is_empty());
}

#[tokio::test]
async fn a_ready_only_job_without_a_pr_and_without_a_pending_open_job_finishes_quietly() {
    let f = Fixture::new().await;

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    assert!(f.env.forge.calls().is_empty());
}

#[tokio::test]
async fn ready_and_summary_go_to_the_pr_recorded_first() {
    let f = Fixture::new().await;
    f.set_summary(f.run, "All done").await;
    // #7 exists on the forge; the job does not see it and creates #8.
    let forge = &f.env.forge;
    forge.push_create_pull_request_result(Ok(pull_request(7)));
    let other = pull_request(7);
    let other = NewPullRequest {
        repo: other.id.repo,
        head: other.head,
        base: other.base,
        title: other.title,
        body: other.body,
        draft: true,
    };
    forge
        .create_pull_request(&f.env.project_id, other)
        .await
        .unwrap();
    forge.push_find_open_pull_request_result(Ok(None));
    forge.push_create_pull_request_result(Ok(pull_request(8)));
    let gate = forge.hold_find_open_pull_request();
    let job = f.job(true, Some(Utc::now()));
    let handler = f.handler();
    let running =
        tokio::spawn(async move { handler.handle(&serde_json::to_value(job).unwrap()).await });
    // The job is in its PR lookup; another creator records #7 before it creates #8.
    tokio::time::timeout(Duration::from_secs(10), async {
        while !f
            .env
            .forge
            .calls()
            .iter()
            .any(|c| matches!(c, ForgeCall::FindOpenPullRequest { .. }))
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET pr_number = 7, pr_url = 'https://x/7'")
        .execute(&f.env.ctx.pool)
        .await
        .unwrap();
    gate.add_permits(8);
    running.await.unwrap().unwrap();

    assert_eq!(f.creates().len(), 2, "the job still created #8");
    assert_eq!(f.pr_number().await, Some(7));
    assert_eq!(f.ready_marks(), [7]);
    assert_eq!(f.comments(), [(7, "All done".to_string())]);
}

#[tokio::test]
async fn a_message_queued_before_the_ready_time_does_not_keep_the_pr_a_draft() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    let ready = Utc::now();
    f.queue_message_at(ready - TimeDelta::minutes(1)).await;

    f.run(f.job(false, Some(ready))).await.unwrap();

    assert_eq!(f.ready_marks(), [1]);
}

#[tokio::test]
async fn a_message_queued_after_the_ready_time_keeps_the_pr_a_draft_but_posts_the_summary() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    f.set_summary(f.run, "All done").await;
    let ready = Utc::now();
    f.queue_message_at(ready + TimeDelta::minutes(1)).await;

    f.run(f.job(false, Some(ready))).await.unwrap();

    assert!(f.ready_marks().is_empty());
    assert_eq!(f.comments(), [(1, "All done".to_string())]);
    assert!(
        f.forge_events()
            .await
            .iter()
            .all(|e| e.2.get("pullRequestReady").is_none())
    );
}

/// Returns once an advisory lock of this database is queued behind another holder.
pub(crate) async fn wait_for_queued_advisory_lock(pool: &PgPool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let queued: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND NOT granted \
                 AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if queued > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("no advisory lock was queued within 10 s");
}

#[tokio::test]
async fn the_ready_update_waits_for_the_task_lock() {
    let f = Fixture::new().await;
    f.run(f.job(true, None)).await.unwrap();
    let lock = lock_task_pr(&f.env.ctx.pool, f.env.task.id).await.unwrap();
    let (handler, job) = (f.handler(), f.job(false, Some(Utc::now())));
    let running =
        tokio::spawn(async move { handler.handle(&serde_json::to_value(job).unwrap()).await });

    wait_for_queued_advisory_lock(&f.env.ctx.pool).await;
    assert!(!running.is_finished());
    assert!(f.ready_marks().is_empty());
    lock.commit().await.unwrap();

    running.await.unwrap().unwrap();
    assert_eq!(f.ready_marks(), [1]);
}

/// A fixture whose task holds the existing branch `fix/login`, attached to `pr` if given.
async fn on_existing_branch(pr: Option<df_db::AttachedPr>) -> Fixture {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    env.task = env.task_on_branch("fix/login", pr).await;
    let run = env.insert_run(env.task.id, 1, "running").await;
    Fixture { env, run }
}

async fn is_attached(f: &Fixture) -> bool {
    sqlx::query_scalar("SELECT pr_attached FROM tasks WHERE id = $1")
        .bind(f.env.task.id.as_uuid())
        .fetch_one(&f.env.ctx.pool)
        .await
        .unwrap()
}

fn attached_pr() -> df_db::AttachedPr {
    df_db::AttachedPr {
        number: 45,
        url: "https://forge.example.test/o/r/pulls/45".into(),
        base: "main".into(),
    }
}

#[tokio::test]
async fn a_task_on_an_existing_branch_opens_its_own_pr_from_that_branch_unattached() {
    let f = on_existing_branch(None).await;

    f.run(f.job(true, None)).await.unwrap();

    let creates = f.creates();
    assert_eq!(
        (creates[0].head.as_str(), creates[0].base.as_str()),
        ("fix/login", "main")
    );
    assert!(!is_attached(&f).await);
}

#[tokio::test]
async fn a_pr_a_human_opened_on_the_existing_branch_is_recorded_as_attached() {
    let f = on_existing_branch(None).await;
    let mut human = pull_request(44);
    human.head = "fix/login".into();
    f.env
        .forge
        .push_find_open_pull_request_result(Ok(Some(human)));

    f.run(f.job(true, None)).await.unwrap();

    assert!(f.creates().is_empty());
    assert_eq!(f.pr_number().await, Some(44));
    assert!(is_attached(&f).await);
}

#[tokio::test]
async fn a_pr_found_on_the_task_own_branch_stays_the_factorys() {
    let f = Fixture::new().await;
    f.env
        .forge
        .push_find_open_pull_request_result(Ok(Some(pull_request(7))));

    f.run(f.job(true, None)).await.unwrap();

    assert!(!is_attached(&f).await);
}

#[tokio::test]
async fn completion_on_an_attached_pr_posts_the_summary_and_changes_nothing_else() {
    let f = on_existing_branch(Some(attached_pr())).await;
    f.set_summary(f.run, "Rebased").await;

    f.run(f.job(false, Some(Utc::now()))).await.unwrap();

    assert!(f.ready_marks().is_empty());
    assert_eq!(f.comments(), [(45, "Rebased".to_string())]);
    assert!(
        !f.env
            .forge
            .calls()
            .iter()
            .any(|c| matches!(c, ForgeCall::UpdatePullRequest { .. }))
    );
}

#[tokio::test]
async fn marking_an_attached_task_done_does_not_call_the_forge() {
    let f = on_existing_branch(Some(attached_pr())).await;
    let job = PrSyncJob {
        run_id: None,
        ..f.job(false, Some(Utc::now()))
    };

    f.run(job).await.unwrap();

    assert!(f.env.forge.calls().is_empty());
}

#[tokio::test]
async fn a_pr_found_after_a_creation_conflict_is_attached_on_an_existing_branch_only() {
    for (existing_branch, attached) in [(true, true), (false, false)] {
        let f = if existing_branch {
            on_existing_branch(None).await
        } else {
            Fixture::new().await
        };
        f.env.forge.push_find_open_pull_request_result(Ok(None));
        f.env
            .forge
            .push_create_pull_request_result(Err(IntegrationError::Conflict("exists".into())));
        f.env
            .forge
            .push_find_open_pull_request_result(Ok(Some(pull_request(9))));

        f.run(f.job(true, None)).await.unwrap();

        assert_eq!(f.pr_number().await, Some(9));
        assert_eq!(
            is_attached(&f).await,
            attached,
            "existing branch: {existing_branch}"
        );
    }
}
