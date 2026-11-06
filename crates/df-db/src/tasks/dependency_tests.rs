//! Tests of creating tasks with a parent, a creator and dependencies, and of the messages and
//! release rules of a task that waits for them.

use df_core::task_transition::{DoneBy, TaskEvent};
use df_core::{DependencyKind, PersonaId, ProjectId, TaskId, TaskKey, TaskStatus, UserId};
use sqlx::PgPool;

use super::test_support::{child_ended_messages, completed_run_ended};
use super::tests::{human, insert_run, new_task, seed_project_and_persona, text};
use super::*;
use crate::task_events::apply_task_event;

struct Env {
    pool: PgPool,
    project: ProjectId,
    persona: PersonaId,
}

async fn env() -> Env {
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

fn depends_on(key: &str, kind: DependencyKind) -> NewDependency {
    NewDependency {
        key: key.parse().unwrap(),
        kind,
    }
}

impl Env {
    async fn create(&self, depends_on: Vec<NewDependency>) -> Result<TaskRow, DbError> {
        let input = NewTask {
            depends_on,
            ..new_task(self.project, self.persona, Some("T"))
        };
        create_task(&self.pool, input).await.map(|(task, _)| task)
    }

    async fn set_times(&self, task: TaskId, completed: bool, done: bool) {
        sqlx::query(
            "UPDATE tasks SET first_completed_at = CASE WHEN $2 THEN now() END,
                 first_done_at = CASE WHEN $3 THEN now() END WHERE id = $1",
        )
        .bind(task.as_uuid())
        .bind(completed)
        .bind(done)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn status(&self, task: TaskId) -> TaskStatus {
        get_task(&self.pool, task).await.unwrap().unwrap().status
    }

    async fn task_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    /// A task of the project with `parent` as its parent and `depends_on`.
    async fn child(&self, parent: TaskId, depends_on: Vec<NewDependency>) -> TaskRow {
        let input = NewTask {
            created_by: Principal::agent(self.persona, "Linus", None, parent),
            parent_id: Some(parent),
            depends_on,
            ..new_task(self.project, self.persona, Some("Child"))
        };
        create_task(&self.pool, input).await.unwrap().0
    }

    /// Makes the assignment prompt of `task` delivered and sets its status.
    async fn settle(&self, task: &TaskRow, status: TaskStatus) {
        let prompts = list_undelivered_messages(&self.pool, task.id)
            .await
            .unwrap();
        let run = insert_run(&self.pool, task.id).await;
        for prompt in prompts {
            mark_prompt_delivered(&self.pool, prompt.id, run)
                .await
                .unwrap();
        }
        crate::task_events::set_status_for_test(&self.pool, task.id, status).await;
    }

    /// Ends a run of `task` as completed, like the agent channel does when the run report arrives.
    async fn end_run_completed(&self, task: TaskId) {
        let run_id = insert_run(&self.pool, task).await;
        let mut tx = self.pool.begin().await.unwrap();
        apply_task_event(
            &mut tx,
            task,
            completed_run_ended(run_id),
            &Principal::system("test"),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
}

#[tokio::test]
async fn a_task_with_an_unsatisfied_dependency_awaits_it_and_keeps_both_dependencies() {
    let env = env().await;
    let first = env.create(Vec::new()).await.unwrap();
    let second = env.create(Vec::new()).await.unwrap();
    env.set_times(second.id, false, true).await;

    let waiting = env
        .create(vec![
            depends_on("DF-1", DependencyKind::Completion),
            depends_on("DF-2", DependencyKind::Done),
        ])
        .await
        .unwrap();

    assert_eq!(waiting.status, TaskStatus::AwaitingDependencies);
    let stored: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT depends_on_task_id, kind FROM task_dependencies WHERE task_id = $1 ORDER BY kind",
    )
    .bind(waiting.id.as_uuid())
    .fetch_all(&env.pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        [
            (first.id.as_uuid(), "completion".to_string()),
            (second.id.as_uuid(), "done".to_string())
        ]
    );
    // The assignment prompt is queued already; the first run receives it.
    let prompts = list_undelivered_messages(&env.pool, waiting.id)
        .await
        .unwrap();
    assert_eq!(prompts.len(), 1);
}

#[tokio::test]
async fn satisfied_dependencies_queue_the_task() {
    let env = env().await;
    let completed = env.create(Vec::new()).await.unwrap();
    env.set_times(completed.id, true, false).await;
    let done = env.create(Vec::new()).await.unwrap();
    env.set_times(done.id, true, true).await;

    let queued = env
        .create(vec![
            depends_on("DF-1", DependencyKind::Completion),
            depends_on("DF-2", DependencyKind::Done),
        ])
        .await
        .unwrap();
    assert_eq!(queued.status, TaskStatus::Queued);

    // Completed is not done: a `done` dependency waits.
    let waiting = env
        .create(vec![depends_on("DF-1", DependencyKind::Done)])
        .await
        .unwrap();
    assert_eq!(waiting.status, TaskStatus::AwaitingDependencies);
}

#[tokio::test]
async fn invalid_dependencies_reject_the_creation_naming_the_key() {
    let env = env().await;
    env.create(Vec::new()).await.unwrap();
    let other: ProjectKey = "OPS".parse().unwrap();
    let (ops, other_persona) = seed_project_and_persona(&env.pool, &other, "Reviewer")
        .await
        .unwrap();
    create_task(&env.pool, new_task(ops, other_persona, Some("Elsewhere")))
        .await
        .unwrap();

    for (dependencies, offending) in [
        (
            vec![depends_on("DF-999", DependencyKind::Completion)],
            "DF-999",
        ),
        (
            vec![depends_on("OPS-1", DependencyKind::Completion)],
            "OPS-1",
        ),
        (
            vec![
                depends_on("DF-1", DependencyKind::Completion),
                depends_on("DF-1", DependencyKind::Done),
            ],
            "DF-1",
        ),
    ] {
        let before = env.task_count().await;
        let err = env.create(dependencies).await.unwrap_err();
        assert!(
            matches!(&err, DbError::InvalidDependency(key) if key == offending),
            "{err}"
        );
        assert_eq!(env.task_count().await, before);
    }
    // The failed attempts left no gap in the numbering.
    let next = env.create(Vec::new()).await.unwrap();
    assert_eq!(next.key.to_string(), "DF-2");
}

#[tokio::test]
async fn creator_and_parent_are_stored_and_read_back() {
    let env = env().await;
    let parent = env.create(Vec::new()).await.unwrap();
    let agent = Principal::agent(env.persona, "Linus", Some(RunId::new()), parent.id);
    let input = NewTask {
        created_by: agent.clone(),
        parent_id: Some(parent.id),
        ..new_task(env.project, env.persona, Some("Child"))
    };
    let (child, prompt) = create_task(&env.pool, input).await.unwrap();

    let loaded = get_task(&env.pool, child.id).await.unwrap().unwrap();
    assert_eq!(
        (loaded.created_by, loaded.parent_id),
        (agent, Some(parent.id))
    );
    // A prompt with a title from the creator has no hidden block.
    assert!(prompt.content.iter().all(|block| !block.hidden));
    let board = get_task(&env.pool, parent.id).await.unwrap().unwrap();
    assert_eq!(board.created_by, Principal::system("test"));
    assert_eq!(board.parent_id, None);
}

#[tokio::test]
async fn a_message_leaves_a_waiting_task_waiting_and_reopens_a_cancelled_one_as_waiting() {
    let env = env().await;
    env.create(Vec::new()).await.unwrap();
    let waiting = env
        .create(vec![depends_on("DF-1", DependencyKind::Completion)])
        .await
        .unwrap();

    queue_message(&env.pool, waiting.id, human(), text("use the new API"))
        .await
        .unwrap();
    assert_eq!(
        env.status(waiting.id).await,
        TaskStatus::AwaitingDependencies
    );

    cancel_task(&env.pool, waiting.id, human()).await.unwrap();
    assert_eq!(env.status(waiting.id).await, TaskStatus::Cancelled);
    queue_message(&env.pool, waiting.id, human(), text("still wanted"))
        .await
        .unwrap();
    assert_eq!(
        env.status(waiting.id).await,
        TaskStatus::AwaitingDependencies
    );
}

#[tokio::test]
async fn a_message_queues_a_task_that_ran_before_even_with_an_unsatisfied_dependency() {
    let env = env().await;
    env.create(Vec::new()).await.unwrap();
    let waiting = env
        .create(vec![depends_on("DF-1", DependencyKind::Completion)])
        .await
        .unwrap();
    insert_run(&env.pool, waiting.id).await;
    cancel_task(&env.pool, waiting.id, human()).await.unwrap();

    queue_message(&env.pool, waiting.id, human(), text("again"))
        .await
        .unwrap();
    assert_eq!(env.status(waiting.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn the_migration_converts_creators_to_principals() {
    let pool = df_test_postgres::empty_pool().await;
    let before = crate::MIGRATOR
        .iter()
        .find(|m| m.description.contains("task dependencies"))
        .map(|m| m.version - 1)
        .unwrap();
    crate::MIGRATOR.run_to(before, &pool).await.unwrap();
    let user = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, issuer, subject, display_name) VALUES ($1, 'i', 's', 'Ada')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let key: ProjectKey = "DF".parse().unwrap();
    let (project, persona) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    for (number, created_by) in [(1_i64, Some(user)), (2, None)] {
        sqlx::query(
            "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id,
                 created_by)
             VALUES ($1, $2, $3, $4, 'T', 'user', $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(project.as_uuid())
        .bind(number)
        .bind(format!("DF-{number}"))
        .bind(persona.as_uuid())
        .bind(created_by)
        .execute(&pool)
        .await
        .unwrap();
    }

    crate::MIGRATOR.run(&pool).await.unwrap();

    let creators: Vec<(String, serde_json::Value)> =
        sqlx::query_as("SELECT key, created_by FROM tasks ORDER BY number")
            .fetch_all(&pool)
            .await
            .unwrap();
    let key = |s: &str| -> TaskKey { s.parse().unwrap() };
    assert_eq!(key(&creators[0].0).number(), 1);
    assert_eq!(
        serde_json::from_value::<Principal>(creators[0].1.clone()).unwrap(),
        Principal::human(UserId::from_uuid(user), "Ada")
    );
    assert_eq!(
        serde_json::from_value::<Principal>(creators[1].1.clone()).unwrap(),
        Principal::system("migration")
    );
}

#[tokio::test]
async fn marking_a_waiting_task_done_completes_it_without_a_run_and_releases_its_dependents() {
    let env = env().await;
    env.create(Vec::new()).await.unwrap();
    let waiting = env
        .create(vec![depends_on("DF-1", DependencyKind::Completion)])
        .await
        .unwrap();
    // No webhook is involved: a board member marks the target done by hand.
    let dependent = env
        .create(vec![depends_on("DF-2", DependencyKind::Done)])
        .await
        .unwrap();
    assert_eq!(dependent.status, TaskStatus::AwaitingDependencies);

    mark_task_done(&env.pool, waiting.id, human())
        .await
        .unwrap();

    assert_eq!(env.status(waiting.id).await, TaskStatus::Completed);
    assert_eq!(env.status(dependent.id).await, TaskStatus::Queued);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE task_id = $1")
        .bind(waiting.id.as_uuid())
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(runs, 0);
}

#[tokio::test]
async fn a_dependent_waits_for_all_its_dependencies() {
    let env = env().await;
    let first = env.create(Vec::new()).await.unwrap();
    env.create(Vec::new()).await.unwrap();
    env.settle(&first, TaskStatus::Waiting).await;
    let dependent = env
        .create(vec![
            depends_on("DF-1", DependencyKind::Done),
            depends_on("DF-2", DependencyKind::Done),
        ])
        .await
        .unwrap();

    mark_task_done(&env.pool, first.id, human()).await.unwrap();
    assert_eq!(
        env.status(dependent.id).await,
        TaskStatus::AwaitingDependencies
    );
}

#[tokio::test]
async fn cancelling_a_waiting_task_discards_its_assignment_and_it_stays_cancelled_on_release() {
    let env = env().await;
    let target = env.create(Vec::new()).await.unwrap();
    env.settle(&target, TaskStatus::Waiting).await;
    let waiting = env
        .create(vec![depends_on("DF-1", DependencyKind::Done)])
        .await
        .unwrap();

    cancel_task(&env.pool, waiting.id, human()).await.unwrap();
    assert!(
        list_undelivered_messages(&env.pool, waiting.id)
            .await
            .unwrap()
            .is_empty()
    );

    mark_task_done(&env.pool, target.id, human()).await.unwrap();
    assert_eq!(env.status(waiting.id).await, TaskStatus::Cancelled);
}

#[tokio::test]
async fn a_child_ending_wakes_the_parent_with_a_message_naming_the_waiting_dependents() {
    let env = env().await;
    let parent = env.create(Vec::new()).await.unwrap();
    env.settle(&parent, TaskStatus::InReview).await;
    let child = env.child(parent.id, Vec::new()).await;
    env.settle(&child, TaskStatus::InReview).await;
    let dependent = env
        .child(parent.id, vec![depends_on("DF-2", DependencyKind::Done)])
        .await;

    cancel_task(&env.pool, child.id, human()).await.unwrap();

    assert_eq!(env.status(parent.id).await, TaskStatus::Queued);
    let messages = child_ended_messages(&env.pool, parent.id).await;
    assert_eq!(messages.len(), 1);
    assert!(
        messages[0].contains("DF-2 \"Child\" was cancelled."),
        "{}",
        messages[0]
    );
    assert!(
        messages[0].contains("Still waiting on DF-2: DF-3"),
        "{}",
        messages[0]
    );
    assert_eq!(
        env.status(dependent.id).await,
        TaskStatus::AwaitingDependencies
    );
}

#[tokio::test]
async fn marking_a_child_done_wakes_the_parent() {
    let env = env().await;
    let parent = env.create(Vec::new()).await.unwrap();
    env.settle(&parent, TaskStatus::Waiting).await;
    let child = env.child(parent.id, Vec::new()).await;
    env.settle(&child, TaskStatus::InReview).await;

    mark_task_done(&env.pool, child.id, human()).await.unwrap();

    let messages = child_ended_messages(&env.pool, parent.id).await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("is done."), "{}", messages[0]);
}

#[tokio::test]
async fn a_parent_that_is_done_gets_no_message() {
    let env = env().await;
    let parent = env.create(Vec::new()).await.unwrap();
    env.settle(&parent, TaskStatus::Waiting).await;
    let child = env.child(parent.id, Vec::new()).await;
    env.settle(&child, TaskStatus::InReview).await;
    mark_task_done(&env.pool, parent.id, human()).await.unwrap();

    cancel_task(&env.pool, child.id, human()).await.unwrap();

    assert!(child_ended_messages(&env.pool, parent.id).await.is_empty());
    assert_eq!(env.status(parent.id).await, TaskStatus::Completed);
}

/// Waits until some other session of the database waits for a lock; fails after 10 seconds.
async fn wait_for_a_lock_wait(pool: &PgPool) {
    let wait = async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE wait_event_type = 'Lock' AND datname = current_database()",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), wait)
        .await
        .expect("no session waited for a lock");
}

#[tokio::test]
async fn the_dependent_is_released_when_its_creation_commits_first() {
    let env = env().await;
    let target = env.create(Vec::new()).await.unwrap();
    env.settle(&target, TaskStatus::Waiting).await;

    // The creation holds a share lock on the target while the board member marks it done.
    let mut creation = env.pool.begin().await.unwrap();
    let input = NewTask {
        depends_on: vec![depends_on("DF-1", DependencyKind::Done)],
        ..new_task(env.project, env.persona, Some("Dependent"))
    };
    let (dependent, _) = create_task_tx(&mut creation, input).await.unwrap();
    assert_eq!(dependent.status, TaskStatus::AwaitingDependencies);
    let pool = env.pool.clone();
    let event = tokio::spawn(async move { mark_task_done(&pool, target.id, human()).await });
    wait_for_a_lock_wait(&env.pool).await;
    creation.commit().await.unwrap();
    event.await.unwrap().unwrap();

    assert_eq!(env.status(dependent.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn the_dependent_is_queued_at_creation_when_the_target_event_commits_first() {
    let env = env().await;
    let target = env.create(Vec::new()).await.unwrap();
    env.settle(&target, TaskStatus::Waiting).await;

    // The event holds the target's row lock while the dependent is created.
    let mut event = env.pool.begin().await.unwrap();
    apply_task_event(
        &mut event,
        target.id,
        TaskEvent::MarkedDone(DoneBy::Board),
        &human(),
    )
    .await
    .unwrap();
    let pool = env.pool.clone();
    let (project, persona) = (env.project, env.persona);
    let creation = tokio::spawn(async move {
        let input = NewTask {
            depends_on: vec![depends_on("DF-1", DependencyKind::Done)],
            ..new_task(project, persona, Some("Dependent"))
        };
        create_task(&pool, input).await
    });
    wait_for_a_lock_wait(&env.pool).await;
    event.commit().await.unwrap();
    let (dependent, _) = creation.await.unwrap().unwrap();

    assert_eq!(dependent.status, TaskStatus::Queued);
}

#[tokio::test]
async fn a_run_completing_after_the_cancel_releases_no_dependent_and_wakes_the_parent_once() {
    let env = env().await;
    let parent = env.create(Vec::new()).await.unwrap();
    env.settle(&parent, TaskStatus::InReview).await;
    let child = env.child(parent.id, Vec::new()).await;
    env.settle(&child, TaskStatus::Running).await;
    let dependent = env
        .child(
            parent.id,
            vec![depends_on("DF-2", DependencyKind::Completion)],
        )
        .await;
    cancel_task(&env.pool, child.id, human()).await.unwrap();

    env.end_run_completed(child.id).await;

    assert_eq!(env.status(child.id).await, TaskStatus::Cancelled);
    assert_eq!(
        env.status(dependent.id).await,
        TaskStatus::AwaitingDependencies
    );
    assert_eq!(child_ended_messages(&env.pool, parent.id).await.len(), 1);
}
