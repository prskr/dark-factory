//! Tests of the task tree queries (`task-tree`): base tasks, the effective base branch, open
//! subtasks and the delegation limits.

use df_core::config::TasksConfig;
use df_core::{PersonaId, ProjectId, TaskStatus};
use sqlx::PgPool;

use super::tests::{new_task, seed_project_and_persona};
use super::*;
use crate::task_views::{
    SubtaskProgress, TaskFilter, get_task_detail, list_subtasks, list_task_items,
};

pub(super) struct Env {
    pub(super) pool: PgPool,
    pub(super) project: ProjectId,
    pub(super) persona: PersonaId,
}

pub(super) async fn env() -> Env {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project, persona) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    Env {
        pool,
        project,
        persona,
    }
}

impl Env {
    /// A task with the given parent, base task and branch (`None`: `factory/task/<key>`).
    pub(super) async fn task(
        &self,
        parent: Option<TaskId>,
        base: Option<&TaskRow>,
        branch: Option<&str>,
    ) -> Result<TaskRow, DbError> {
        let mut input = new_task(self.project, self.persona, Some("T"));
        input.parent_id = parent;
        let placement = TaskPlacement {
            branch: branch.map(|b| ExistingBranch::parse(b, "main").unwrap()),
            base_task: base.map(|b| b.key.clone()),
            ..TaskPlacement::default()
        };
        create_task_on(&self.pool, input, placement)
            .await
            .map(|(task, _)| task)
    }

    pub(super) async fn set_status(&self, task: &TaskRow, status: TaskStatus) {
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(task.id.as_uuid())
            .bind(status.as_str())
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn set_pr(&self, task: &TaskRow) {
        sqlx::query("UPDATE tasks SET pr_number = 5, pr_url = 'https://forge/pr/5' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn base_branch(&self, task: &TaskRow) -> String {
        base_branch(&self.pool, task.id).await.unwrap()
    }
}

// Scenario "No base task".
#[tokio::test]
async fn a_task_without_base_task_has_the_project_base_branch() {
    let env = env().await;
    let task = env.task(None, None, None).await.unwrap();
    assert_eq!(env.base_branch(&task).await, "main");
}

// Scenario "Based on a task": the branch comes from `tasks.branch`, not from the key.
#[tokio::test]
async fn a_task_based_on_an_open_task_starts_from_that_tasks_branch() {
    let env = env().await;
    let base = env.task(None, None, None).await.unwrap();
    let on_existing = env.task(None, None, Some("fix/login")).await.unwrap();

    let first = env.task(None, Some(&base), None).await.unwrap();
    let second = env.task(None, Some(&on_existing), None).await.unwrap();

    assert_eq!(env.base_branch(&first).await, "factory/task/DF-1");
    assert_eq!(env.base_branch(&second).await, "fix/login");
}

// Scenario "Base task merged".
#[tokio::test]
async fn a_done_base_task_is_replaced_by_its_own_base() {
    let env = env().await;
    let base = env.task(None, None, None).await.unwrap();
    let task = env.task(None, Some(&base), None).await.unwrap();

    env.set_status(&base, TaskStatus::Completed).await;

    assert_eq!(env.base_branch(&task).await, "main");
}

#[tokio::test]
async fn a_chain_of_two_done_base_tasks_ends_at_the_project_base() {
    let env = env().await;
    let first = env.task(None, None, None).await.unwrap();
    let second = env.task(None, Some(&first), None).await.unwrap();
    let third = env.task(None, Some(&second), None).await.unwrap();

    env.set_status(&first, TaskStatus::Completed).await;
    assert_eq!(env.base_branch(&third).await, "factory/task/DF-2");
    env.set_status(&second, TaskStatus::Completed).await;
    assert_eq!(env.base_branch(&third).await, "main");
}

#[tokio::test]
async fn a_done_base_task_hands_over_to_the_open_base_task_behind_it() {
    let env = env().await;
    let first = env.task(None, None, None).await.unwrap();
    let second = env.task(None, Some(&first), None).await.unwrap();
    let third = env.task(None, Some(&second), None).await.unwrap();

    env.set_status(&second, TaskStatus::Completed).await;

    assert_eq!(env.base_branch(&third).await, "factory/task/DF-1");
}

#[tokio::test]
async fn the_base_of_an_attached_pr_applies_without_a_base_task() {
    let env = env().await;
    let task = env.task(None, None, None).await.unwrap();
    sqlx::query("UPDATE tasks SET base_branch = 'release/1.2' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&env.pool)
        .await
        .unwrap();
    assert_eq!(env.base_branch(&task).await, "release/1.2");
}

#[tokio::test]
async fn the_base_task_is_stored_with_the_task() {
    let env = env().await;
    let base = env.task(None, None, None).await.unwrap();
    let task = env.task(Some(base.id), Some(&base), None).await.unwrap();
    assert_eq!(task.base_task_id, Some(base.id));
    assert_eq!(task.parent_id, Some(base.id));
    assert_eq!(get_task(&env.pool, task.id).await.unwrap().unwrap(), task);
}

// Scenarios "Base task already done" and "Unknown base task".
#[tokio::test]
async fn a_base_task_that_is_done_failed_cancelled_or_unknown_is_rejected() {
    let env = env().await;
    for status in [
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ] {
        let base = env.task(None, None, None).await.unwrap();
        env.set_status(&base, status).await;
        let err = env.task(None, Some(&base), None).await.unwrap_err();
        assert!(
            matches!(&err, DbError::InvalidBaseTask(key) if *key == base.key.to_string()),
            "{status}: {err:?}"
        );
    }
    let mut unknown = new_task(env.project, env.persona, Some("T"));
    unknown.parent_id = None;
    let placement = TaskPlacement {
        base_task: Some("DF-999".parse().unwrap()),
        ..TaskPlacement::default()
    };
    let err = create_task_on(&env.pool, unknown, placement)
        .await
        .unwrap_err();
    assert!(matches!(&err, DbError::InvalidBaseTask(key) if key == "DF-999"));
}

#[tokio::test]
async fn a_rejected_base_task_creates_nothing() {
    let env = env().await;
    let base = env.task(None, None, None).await.unwrap();
    env.set_status(&base, TaskStatus::Completed).await;
    env.task(None, Some(&base), None).await.unwrap_err();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

async fn open_keys(env: &Env, parent: &TaskRow) -> Vec<String> {
    open_subtasks(&env.pool, parent.id)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.key.to_string())
        .collect()
}

// Scenario "Research subtask finished".
#[tokio::test]
async fn a_finished_subtask_without_pr_is_not_open() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    for status in TaskStatus::SUBTASK_FINISHED {
        env.set_status(&child, *status).await;
        assert!(open_keys(&env, &parent).await.is_empty(), "{status}");
    }
}

// Scenario "Subtask waiting for a human".
#[tokio::test]
async fn a_subtask_in_any_other_status_is_open() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    for status in [
        TaskStatus::AwaitingDependencies,
        TaskStatus::Queued,
        TaskStatus::Running,
        TaskStatus::Waiting,
        TaskStatus::Blocked,
        TaskStatus::RateLimited,
        TaskStatus::LimitReached,
    ] {
        env.set_status(&child, status).await;
        assert_eq!(open_keys(&env, &parent).await, [child.key.to_string()]);
    }
}

// Scenario "Iteration with an unmerged PR".
#[tokio::test]
async fn an_iteration_with_an_unmerged_pr_is_open_until_done_or_cancelled() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let iteration = env
        .task(Some(parent.id), Some(&parent), None)
        .await
        .unwrap();
    env.set_status(&iteration, TaskStatus::InReview).await;
    assert!(open_keys(&env, &parent).await.is_empty(), "no PR yet");

    env.set_pr(&iteration).await;
    let open = open_subtasks(&env.pool, parent.id).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].pr_url.as_deref(), Some("https://forge/pr/5"));

    for status in [TaskStatus::Completed, TaskStatus::Cancelled] {
        env.set_status(&iteration, status).await;
        assert!(open_keys(&env, &parent).await.is_empty(), "{status}");
    }
}

#[tokio::test]
async fn a_subtask_with_a_pr_into_another_base_is_not_held_back_by_it() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    env.set_status(&child, TaskStatus::InReview).await;
    env.set_pr(&child).await;
    assert!(open_keys(&env, &parent).await.is_empty());
}

#[tokio::test]
async fn only_direct_subtasks_count_and_they_are_listed_by_number() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let first = env.task(Some(parent.id), None, None).await.unwrap();
    let second = env.task(Some(parent.id), None, None).await.unwrap();
    env.task(Some(first.id), None, None).await.unwrap();
    assert_eq!(
        open_keys(&env, &parent).await,
        [first.key.to_string(), second.key.to_string()]
    );
}

fn limits(max_depth: u32, max_open_subtasks: u32) -> TasksConfig {
    TasksConfig {
        max_depth,
        max_open_subtasks,
    }
}

impl Env {
    async fn limited(
        &self,
        parent: &TaskRow,
        limits: impl Into<Option<TasksConfig>>,
    ) -> Result<TaskRow, DbError> {
        let mut input = new_task(self.project, self.persona, Some("T"));
        input.parent_id = Some(parent.id);
        let placement = TaskPlacement {
            limits: limits.into(),
            ..TaskPlacement::default()
        };
        create_task_on(&self.pool, input, placement)
            .await
            .map(|(task, _)| task)
    }
}

// Scenario "Too deep".
#[tokio::test]
async fn a_task_with_as_many_ancestors_as_the_limit_cannot_create_a_subtask() {
    let env = env().await;
    let root = env.task(None, None, None).await.unwrap();
    let child = env.limited(&root, limits(2, 10)).await.unwrap();
    // `child` has one ancestor, so its subtask would have two: allowed.
    let grandchild = env.limited(&child, limits(2, 10)).await.unwrap();
    // `grandchild` has two ancestors, its subtask would have three.
    let err = env.limited(&grandchild, limits(2, 10)).await.unwrap_err();
    assert!(
        matches!(err, DbError::DelegationLimit(DelegationLimit::Depth(2))),
        "{err:?}"
    );
    assert!(err.to_string().contains("tasks.maxDepth"));
}

// Scenario "Too many open subtasks".
#[tokio::test]
async fn a_task_with_as_many_open_subtasks_as_the_limit_cannot_create_another() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    for _ in 0..3 {
        env.limited(&parent, limits(5, 3)).await.unwrap();
    }
    let err = env.limited(&parent, limits(5, 3)).await.unwrap_err();
    assert!(
        matches!(
            err,
            DbError::DelegationLimit(DelegationLimit::OpenSubtasks(3))
        ),
        "{err:?}"
    );
    assert!(err.to_string().contains("tasks.maxOpenSubtasks"));
}

// Scenario "Finished subtasks don't count".
#[tokio::test]
async fn finished_subtasks_do_not_count_towards_the_limit() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    for _ in 0..3 {
        let child = env.limited(&parent, limits(5, 3)).await.unwrap();
        env.set_status(&child, TaskStatus::InReview).await;
    }
    assert!(env.limited(&parent, limits(5, 3)).await.is_ok());
}

#[tokio::test]
async fn creation_without_limits_is_unbounded() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    for _ in 0..4 {
        env.limited(&parent, None).await.unwrap();
    }
}

async fn progress_of(env: &Env, task: &TaskRow) -> SubtaskProgress {
    let items = list_task_items(&env.pool, &TaskFilter::default())
        .await
        .unwrap();
    items
        .into_iter()
        .find(|t| t.key == task.key)
        .unwrap()
        .subtasks
}

// Scenario "Partial progress" of `task-ui`: done is in review or done, total leaves out the
// cancelled.
#[tokio::test]
async fn the_task_list_counts_done_and_total_direct_subtasks() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    assert_eq!(
        progress_of(&env, &parent).await,
        SubtaskProgress { done: 0, total: 0 }
    );

    for status in [
        TaskStatus::InReview,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ] {
        let child = env.task(Some(parent.id), None, None).await.unwrap();
        env.set_status(&child, status).await;
    }
    // A grandchild does not count towards the parent.
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    env.task(Some(child.id), None, None).await.unwrap();

    assert_eq!(
        progress_of(&env, &parent).await,
        SubtaskProgress { done: 2, total: 4 }
    );
    assert_eq!(
        progress_of(&env, &child).await,
        SubtaskProgress { done: 0, total: 1 }
    );
}

// Scenario "Feature task": the Subtasks tab lists the direct subtasks with their own progress.
#[tokio::test]
async fn the_subtasks_of_a_task_are_listed_by_number_with_their_status_and_progress() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let done = env.task(Some(parent.id), None, None).await.unwrap();
    let running = env.task(Some(parent.id), None, None).await.unwrap();
    env.task(Some(running.id), None, None).await.unwrap();
    env.set_status(&done, TaskStatus::Completed).await;
    env.set_status(&running, TaskStatus::Running).await;

    let subtasks = list_subtasks(&env.pool, parent.id).await.unwrap();

    let shown: Vec<_> = subtasks
        .iter()
        .map(|s| (s.key.as_str(), s.status, s.subtasks))
        .collect();
    assert_eq!(
        shown,
        [
            ("DF-2", TaskStatus::Completed, SubtaskProgress::default()),
            (
                "DF-3",
                TaskStatus::Running,
                SubtaskProgress { done: 0, total: 1 }
            ),
        ]
    );
    assert!(list_subtasks(&env.pool, done.id).await.unwrap().is_empty());
}

// Scenario "Base task link".
#[tokio::test]
async fn the_task_detail_names_the_base_task() {
    let env = env().await;
    let base = env.task(None, None, None).await.unwrap();
    let task = env.task(None, Some(&base), None).await.unwrap();

    let detail = get_task_detail(&env.pool, task.id).await.unwrap().unwrap();
    let plain = get_task_detail(&env.pool, base.id).await.unwrap().unwrap();

    assert_eq!(
        (
            detail.base_task_key.as_deref(),
            detail.base_task_title.as_deref()
        ),
        (Some("DF-1"), Some("T"))
    );
    assert_eq!(plain.base_task_key, None);
}

/// The tasks of the `task_changed` notifications received, until `done` accepts the list.
async fn task_changes_until(
    rx: &mut tokio::sync::broadcast::Receiver<Notification>,
    done: impl Fn(&[TaskId]) -> bool,
) -> Vec<TaskId> {
    let mut changed = Vec::new();
    while !done(&changed) {
        let note = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("a notification within 5 s")
            .expect("the notifier is open");
        if let Notification::TaskChanged { task_id } = note {
            changed.push(task_id);
        }
    }
    changed
}

// The Subtasks tab of the parent shows a new subtask at once.
#[tokio::test]
async fn creating_a_subtask_publishes_a_change_of_its_parent() {
    let env = env().await;
    let notifier = crate::notify::Notifier::start(&env.pool).await.unwrap();
    let mut rx = notifier.subscribe();
    let parent = env.task(None, None, None).await.unwrap();
    env.task(Some(parent.id), None, None).await.unwrap();

    // The parent's own creation, the child's, and the parent again.
    let seen = task_changes_until(&mut rx, |seen| seen.len() == 3).await;

    assert_eq!(
        seen.iter().filter(|id| **id == parent.id).count(),
        2,
        "{seen:?}"
    );
}

// A transition of a subtask that changes its status publishes the parent too.
#[tokio::test]
async fn a_status_change_of_a_subtask_publishes_a_change_of_its_parent() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    let notifier = crate::notify::Notifier::start(&env.pool).await.unwrap();
    let mut rx = notifier.subscribe();

    cancel_task(&env.pool, child.id, tests::human())
        .await
        .unwrap();

    let seen = task_changes_until(&mut rx, |seen| seen.contains(&parent.id)).await;
    assert!(
        seen.contains(&child.id) && seen.contains(&parent.id),
        "{seen:?}"
    );
}

impl Env {
    /// Enqueues the job that opens the draft PR of `task`; `done` marks it finished.
    async fn open_job(&self, task: &TaskRow, done: bool) {
        let payload = serde_json::to_value(PrSyncJob {
            task_id: task.id,
            run_id: None,
            open: true,
            ready: None,
        })
        .unwrap();
        sqlx::query(
            "INSERT INTO outbox (kind, payload, done_at) VALUES ('pr.sync', $1, CASE WHEN $2 THEN now() END)",
        )
        .bind(payload)
        .bind(done)
        .execute(&self.pool)
        .await
        .unwrap();
    }
}

// The run end wakes the parent before the job that opens the PR ran.
#[tokio::test]
async fn an_iteration_whose_pr_is_about_to_open_is_open_until_it_is_done_or_cancelled() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let iteration = env
        .task(Some(parent.id), Some(&parent), None)
        .await
        .unwrap();
    env.set_status(&iteration, TaskStatus::InReview).await;
    assert!(open_keys(&env, &parent).await.is_empty(), "no job yet");

    env.open_job(&iteration, false).await;
    assert_eq!(open_keys(&env, &parent).await, [iteration.key.to_string()]);

    for status in [TaskStatus::Completed, TaskStatus::Cancelled] {
        env.set_status(&iteration, status).await;
        assert!(open_keys(&env, &parent).await.is_empty(), "{status}");
    }
}

#[tokio::test]
async fn a_finished_open_job_without_a_pr_no_longer_holds_the_parent_back() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let iteration = env
        .task(Some(parent.id), Some(&parent), None)
        .await
        .unwrap();
    env.set_status(&iteration, TaskStatus::InReview).await;
    env.open_job(&iteration, true).await;
    assert!(open_keys(&env, &parent).await.is_empty());
}

#[tokio::test]
async fn a_subtask_not_based_on_the_parent_ignores_a_pending_open_job() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    env.set_status(&child, TaskStatus::InReview).await;
    env.open_job(&child, false).await;
    assert!(open_keys(&env, &parent).await.is_empty());
}

// Scenario "Board member marks done".
#[tokio::test]
async fn a_board_member_can_mark_a_task_with_open_subtasks_done() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    env.task(Some(parent.id), None, None).await.unwrap();
    env.set_status(&parent, TaskStatus::Waiting).await;

    mark_task_done(&env.pool, parent.id, tests::human())
        .await
        .unwrap();

    assert_eq!(
        get_task(&env.pool, parent.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Completed
    );
}

// The gateway's route and the workspace must name the same base branch.
#[tokio::test]
async fn the_egress_snapshot_resolves_the_same_base_branch_as_base_branch() {
    let env = env().await;
    let plain = env.task(None, None, None).await.unwrap();
    let open_base = env.task(None, None, None).await.unwrap();
    let on_open = env.task(None, Some(&open_base), None).await.unwrap();
    let first = env.task(None, None, None).await.unwrap();
    let second = env.task(None, Some(&first), None).await.unwrap();
    let third = env.task(None, Some(&second), None).await.unwrap();
    env.set_status(&first, TaskStatus::Completed).await;
    env.set_status(&second, TaskStatus::Completed).await;
    let tasks = [&plain, &on_open, &third];
    for task in tasks {
        sqlx::query("INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, 1, 'running', '10.0.0.1')")
            .bind(Uuid::now_v7())
            .bind(task.id.as_uuid())
            .execute(&env.pool)
            .await
            .unwrap();
    }

    let routes = crate::egress::active_runs(&env.pool).await.unwrap();

    assert_eq!(routes.len(), 3);
    for task in tasks {
        let route = routes.iter().find(|r| r.task_key == task.key.to_string());
        assert_eq!(
            route.map(|r| r.base_branch.clone()),
            Some(env.base_branch(task).await),
            "{}",
            task.key
        );
    }
}

#[tokio::test]
async fn a_stored_pull_request_number_out_of_range_is_invalid_data() {
    let env = env().await;
    let task = env.task(None, None, None).await.unwrap();
    sqlx::query("UPDATE tasks SET pr_number = -1 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&env.pool)
        .await
        .unwrap();

    let err = subtask_to_merge(&env.pool, env.project, &task.key)
        .await
        .unwrap_err();

    assert!(matches!(err, DbError::InvalidData(_)), "{err:?}");
}

#[tokio::test]
async fn a_task_whose_subtasks_are_all_cancelled_shows_no_progress() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(parent.id), None, None).await.unwrap();
    env.set_status(&child, TaskStatus::Cancelled).await;
    assert_eq!(
        progress_of(&env, &parent).await,
        SubtaskProgress { done: 0, total: 0 }
    );
}

// Scenario "Existing projects".
#[tokio::test]
async fn existing_projects_keep_subtask_merges_with_humans() {
    let pool = df_test_postgres::empty_pool().await;
    let before = crate::MIGRATOR
        .iter()
        .find(|m| m.description.contains("task tree"))
        .map(|m| m.version - 1)
        .unwrap();
    crate::MIGRATOR.run_to(before, &pool).await.unwrap();
    let key: ProjectKey = "DF".parse().unwrap();
    seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();

    crate::MIGRATOR.run(&pool).await.unwrap();

    let merge: String = sqlx::query_scalar("SELECT subtask_merge FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(merge, "human");
}
