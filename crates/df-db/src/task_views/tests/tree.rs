//! The task tree in the list: scopes, "needs you" and ancestors (`task-list-tree`).

use df_core::{PersonaId, ProjectId};

use super::*;
use crate::tasks::{PR_SYNC_KIND, PrSyncJob};

/// The tasks of one test: `DF-1` from `seed` (queued, top level) and what a test adds.
struct Tree {
    pool: PgPool,
    top_level: TaskId,
    project: ProjectId,
    persona: PersonaId,
}

impl Tree {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (top_level, _) = seed(&pool).await;
        let (project, persona): (Uuid, Uuid) =
            sqlx::query_as("SELECT project_id, persona_id FROM tasks")
                .fetch_one(&pool)
                .await
                .unwrap();
        Self {
            pool,
            top_level,
            project: ProjectId::from_uuid(project),
            persona: PersonaId::from_uuid(persona),
        }
    }

    /// A task below `parent` (top level without one) in `status`.
    async fn add(&self, parent: Option<TaskId>, status: TaskStatus) -> TaskId {
        let (task, _) = crate::create_task(
            &self.pool,
            crate::NewTask {
                project_id: self.project,
                persona_id: self.persona,
                title: Some("Task".into()),
                description: String::new(),
                issue_number: None,
                created_by: Principal::system("test"),
                parent_id: parent,
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
        self.set_status(task.id, status).await;
        task.id
    }

    async fn add_chat(&self) -> TaskId {
        crate::chats::create_chat(
            &self.pool,
            crate::chats::NewChat {
                project_id: self.project,
                persona_id: self.persona,
                author: Principal::system("test"),
                text: "Hello".into(),
            },
        )
        .await
        .unwrap()
        .0
        .id
    }

    async fn set_status(&self, task: TaskId, status: TaskStatus) {
        crate::task_events::set_status_for_test(&self.pool, task, status).await;
    }

    async fn set_pr(&self, task: TaskId) {
        sqlx::query("UPDATE tasks SET pr_number = 5 WHERE id = $1")
            .bind(task.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn set_base(&self, task: TaskId, base: TaskId) {
        sqlx::query("UPDATE tasks SET base_task_id = $2 WHERE id = $1")
            .bind(task.as_uuid())
            .bind(base.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn parent_agent_merges(&self) {
        sqlx::query("UPDATE projects SET subtask_merge = 'parent-agent'")
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn enqueue_pr_open_job(&self, task: TaskId) {
        let job = PrSyncJob {
            task_id: task,
            run_id: None,
            open: true,
            ready: None,
        };
        let mut conn = self.pool.acquire().await.unwrap();
        crate::outbox::enqueue(&mut conn, PR_SYNC_KIND, &serde_json::to_value(job).unwrap())
            .await
            .unwrap();
    }

    async fn finish_pr_jobs(&self) {
        sqlx::query("UPDATE outbox SET done_at = now() WHERE kind = $1")
            .bind(PR_SYNC_KIND)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn key(&self, task: TaskId) -> String {
        sqlx::query_scalar("SELECT key FROM tasks WHERE id = $1")
            .bind(task.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn keys(&self, filter: &TaskFilter) -> Vec<String> {
        list_task_items(&self.pool, filter)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.key.to_string())
            .collect()
    }

    /// The row of `task` in the unscoped list.
    async fn item(&self, task: TaskId) -> TaskListItem {
        list_task_items(&self.pool, &TaskFilter::default())
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == task.as_uuid())
            .unwrap()
    }

    async fn needs_you(&self, task: TaskId) -> bool {
        self.item(task).await.needs_you
    }

    async fn ancestor_keys(&self, task: TaskId) -> Vec<String> {
        ancestors(&self.pool, task)
            .await
            .unwrap()
            .into_iter()
            .map(|ancestor| ancestor.key.to_string())
            .collect()
    }
}

fn scoped(scope: ListScope) -> TaskFilter {
    TaskFilter {
        scope: Some(scope),
        ..TaskFilter::default()
    }
}

#[tokio::test]
async fn the_top_level_scope_lists_tasks_without_parent_or_with_a_chat_parent_not_subtasks() {
    let tree = Tree::new().await;
    let chat = tree.add_chat().await;
    let from_chat = tree.add(Some(chat), TaskStatus::Queued).await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Queued).await;

    let top = tree.keys(&scoped(ListScope::TopLevel)).await;

    assert!(top.contains(&tree.key(tree.top_level).await));
    assert!(top.contains(&tree.key(from_chat).await));
    assert!(!top.contains(&tree.key(subtask).await));
    assert_eq!(top.len(), 2, "the chat itself is no task: {top:?}");
}

#[tokio::test]
async fn a_filter_without_scope_still_lists_subtasks() {
    let tree = Tree::new().await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Waiting).await;
    let filter = TaskFilter {
        group: Some(StatusSet::Waiting),
        ..TaskFilter::default()
    };

    assert_eq!(tree.keys(&filter).await, [tree.key(subtask).await]);
}

#[tokio::test]
async fn the_subtasks_scope_lists_direct_subtasks_in_sort_order_ignoring_status_and_project() {
    let tree = Tree::new().await;
    let first = tree.add(Some(tree.top_level), TaskStatus::Completed).await;
    let second = tree.add(Some(tree.top_level), TaskStatus::Running).await;
    tree.add(Some(first), TaskStatus::Running).await;
    tree.add(None, TaskStatus::Running).await;
    let filter = TaskFilter {
        group: Some(StatusSet::Queued),
        project_key: Some("WEB".into()),
        scope: Some(ListScope::SubtasksOf(tree.top_level)),
        sort: TaskSort::Created,
    };

    // Newest created first; the subtask of a subtask and the other top-level task are not listed.
    assert_eq!(
        tree.keys(&filter).await,
        [tree.key(second).await, tree.key(first).await]
    );
}

#[tokio::test]
async fn the_task_scope_lists_the_one_task_with_that_key_whatever_its_level() {
    let tree = Tree::new().await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Completed).await;
    let key = tree.key(subtask).await;
    let filter = TaskFilter {
        group: Some(StatusSet::Active),
        scope: Some(ListScope::Task(key.parse().unwrap())),
        ..TaskFilter::default()
    };

    assert_eq!(tree.keys(&filter).await, [key]);
    let unknown = scoped(ListScope::Task("DF-99".parse().unwrap()));
    assert!(tree.keys(&unknown).await.is_empty());
}

#[tokio::test]
async fn waiting_blocked_and_limit_reached_need_you_also_two_levels_down() {
    let tree = Tree::new().await;
    tree.add(Some(tree.top_level), TaskStatus::Running).await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Running).await;
    tree.add(Some(subtask), TaskStatus::Waiting).await;
    let nested = tree.add(Some(subtask), TaskStatus::Running).await;
    tree.add(Some(nested), TaskStatus::Blocked).await;
    tree.add(Some(nested), TaskStatus::LimitReached).await;
    for quiet in [
        TaskStatus::Running,
        TaskStatus::RateLimited,
        TaskStatus::Cancelled,
    ] {
        tree.add(Some(nested), quiet).await;
    }

    let top_level = tree.item(tree.top_level).await;
    let subtask = tree.item(subtask).await;
    let nested = tree.item(nested).await;

    assert_eq!((top_level.needs_you, top_level.needs_you_below), (false, 3));
    assert_eq!((subtask.needs_you, subtask.needs_you_below), (false, 3));
    assert_eq!((nested.needs_you, nested.needs_you_below), (false, 2));
    let waiting = tree.add(None, TaskStatus::Waiting).await;
    assert!(tree.needs_you(waiting).await);
}

#[tokio::test]
async fn a_subtask_in_review_with_a_pr_needs_you_when_humans_merge() {
    let tree = Tree::new().await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.set_pr(subtask).await;

    assert!(tree.needs_you(subtask).await);
    assert_eq!(tree.item(tree.top_level).await.needs_you_below, 1);
}

#[tokio::test]
async fn a_subtask_in_review_that_its_parent_agent_merges_does_not_need_you() {
    let tree = Tree::new().await;
    tree.parent_agent_merges().await;
    tree.set_status(tree.top_level, TaskStatus::Running).await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.set_pr(subtask).await;
    tree.set_base(subtask, tree.top_level).await;

    assert!(!tree.needs_you(subtask).await);

    // The parent agent is gone.
    tree.set_status(tree.top_level, TaskStatus::Cancelled).await;
    assert!(tree.needs_you(subtask).await);
}

#[tokio::test]
async fn a_subtask_on_another_base_task_needs_you_even_if_the_parent_agent_merges() {
    let tree = Tree::new().await;
    tree.parent_agent_merges().await;
    tree.set_status(tree.top_level, TaskStatus::Running).await;
    let other = tree.add(None, TaskStatus::Running).await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.set_pr(subtask).await;
    tree.set_base(subtask, other).await;

    assert!(tree.needs_you(subtask).await);
}

#[tokio::test]
async fn a_subtask_in_review_needs_you_only_with_a_pr_or_a_pending_pr_open_job() {
    let tree = Tree::new().await;
    let research = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    let queued_pr = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.enqueue_pr_open_job(queued_pr).await;

    assert!(!tree.needs_you(research).await);
    assert!(tree.needs_you(queued_pr).await);
}

#[tokio::test]
async fn a_top_level_task_in_review_needs_you_without_a_pr() {
    let tree = Tree::new().await;
    let top = tree.add(None, TaskStatus::InReview).await;

    assert!(tree.needs_you(top).await);
}

#[tokio::test]
async fn an_in_review_task_created_by_a_chat_needs_you_without_a_pr_or_pr_job() {
    let tree = Tree::new().await;
    let chat = tree.add_chat().await;
    let from_chat = tree.add(Some(chat), TaskStatus::InReview).await;

    assert!(tree.needs_you(from_chat).await);
}

#[tokio::test]
async fn a_task_whose_subtasks_are_all_cancelled_still_has_subtasks() {
    let tree = Tree::new().await;
    let cancelled = tree.add(Some(tree.top_level), TaskStatus::Cancelled).await;
    let alone = tree.add(None, TaskStatus::Queued).await;

    let item = tree.item(tree.top_level).await;

    assert_eq!((item.has_subtasks, item.subtasks.total), (true, 0));
    assert!(!tree.item(alone).await.has_subtasks);
    assert!(!tree.item(cancelled).await.has_subtasks);
}

#[tokio::test]
async fn the_pr_and_pending_job_rule_gives_the_same_answers_in_all_three_queries() {
    let tree = Tree::new().await;
    let with_pr = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.set_pr(with_pr).await;
    let with_job = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.enqueue_pr_open_job(with_job).await;
    let research = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    let finished_job = tree.add(Some(tree.top_level), TaskStatus::InReview).await;
    tree.enqueue_pr_open_job(finished_job).await;
    tree.finish_pr_jobs().await;
    let all = [with_pr, with_job, research, finished_job];
    for task in all {
        tree.set_base(task, tree.top_level).await;
    }
    // Enqueued after the others finished: only this job is pending.
    tree.enqueue_pr_open_job(with_job).await;

    let open: Vec<String> = crate::open_subtasks(&tree.pool, tree.top_level)
        .await
        .unwrap()
        .into_iter()
        .map(|subtask| subtask.key.to_string())
        .collect();
    let mut needing = Vec::new();
    let mut with_pending_job = Vec::new();
    for task in all {
        if tree.needs_you(task).await {
            needing.push(tree.key(task).await);
        }
        if crate::outbox::has_unfinished_pr_open_job(&tree.pool, task)
            .await
            .unwrap()
        {
            with_pending_job.push(task);
        }
    }

    assert_eq!(needing, open);
    assert_eq!(open, [tree.key(with_pr).await, tree.key(with_job).await]);
    assert_eq!(with_pending_job, [with_job]);
}

#[tokio::test]
async fn active_keeps_a_finished_top_level_task_while_a_task_below_needs_you() {
    let tree = Tree::new().await;
    tree.set_status(tree.top_level, TaskStatus::Completed).await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Waiting).await;
    let key = tree.key(tree.top_level).await;
    let by = |group| TaskFilter {
        group: Some(group),
        scope: Some(ListScope::TopLevel),
        ..TaskFilter::default()
    };

    assert_eq!(tree.keys(&by(StatusSet::Active)).await, [key.as_str()]);
    assert_eq!(tree.keys(&by(StatusSet::Done)).await, [key.as_str()]);

    tree.set_status(subtask, TaskStatus::Running).await;
    assert!(tree.keys(&by(StatusSet::Active)).await.is_empty());
    assert_eq!(tree.keys(&by(StatusSet::Done)).await, [key]);
}

#[tokio::test]
async fn ancestors_list_the_parent_and_up_to_the_top_level_task() {
    let tree = Tree::new().await;
    let subtask = tree.add(Some(tree.top_level), TaskStatus::Running).await;
    let nested = tree.add(Some(subtask), TaskStatus::Running).await;

    assert_eq!(
        tree.ancestor_keys(nested).await,
        [tree.key(subtask).await, tree.key(tree.top_level).await]
    );
    assert!(tree.ancestor_keys(tree.top_level).await.is_empty());
    assert!(tree.ancestor_keys(TaskId::new()).await.is_empty());
}

#[tokio::test]
async fn ancestors_stop_below_a_chat_parent() {
    let tree = Tree::new().await;
    let chat = tree.add_chat().await;
    let top = tree.add(Some(chat), TaskStatus::Running).await;
    let subtask = tree.add(Some(top), TaskStatus::Running).await;

    assert_eq!(tree.ancestor_keys(subtask).await, [tree.key(top).await]);
}
