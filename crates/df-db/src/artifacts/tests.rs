//! Tests of the artifact storage, the related-task rule and the artifacts in the child-ended
//! message.

use std::io::Cursor;

use df_core::artifacts::{ArtifactError, check_artifact};
use df_core::{ArtifactsConfig, DependencyKind, PersonaId, ProjectId, RunId, TaskId};
use image::{ImageFormat, RgbImage};
use sqlx::PgPool;

use crate::artifacts::{
    NewArtifact, WriteArtifactError, list_artifacts, read_artifact_content, readable_task,
    write_artifact,
};
use crate::tasks::test_support::child_ended_messages;
use crate::tasks::tests::{human, new_task, seed_project_and_persona};
use crate::tasks::{NewDependency, NewTask, TaskRow, cancel_task, create_task};
use df_core::ProjectKey;

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

impl Env {
    async fn task(&self, parent: Option<TaskId>, depends_on: &[&str]) -> TaskRow {
        let input = NewTask {
            parent_id: parent,
            depends_on: depends_on
                .iter()
                .map(|key| NewDependency {
                    key: key.parse().unwrap(),
                    kind: DependencyKind::Completion,
                })
                .collect(),
            ..new_task(self.project, self.persona, Some("T"))
        };
        create_task(&self.pool, input).await.unwrap().0
    }

    /// The run `write` records its events under; created on first use.
    async fn run(&self, task: TaskId) -> RunId {
        sqlx::query(
            "INSERT INTO runs (id, task_id, number) VALUES ($1, $2, 1) ON CONFLICT DO NOTHING",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(task.as_uuid())
        .execute(&self.pool)
        .await
        .unwrap();
        let id = sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1 AND number = 1")
            .bind(task.as_uuid())
            .fetch_one(&self.pool)
            .await
            .unwrap();
        RunId::from_uuid(id)
    }

    async fn write(
        &self,
        task: TaskId,
        name: &str,
        bytes: &[u8],
        max_per_task: u32,
    ) -> Result<super::ArtifactMeta, WriteArtifactError> {
        self.write_call(
            task,
            name,
            bytes,
            max_per_task,
            &uuid::Uuid::now_v7().to_string(),
        )
        .await
    }

    async fn write_call(
        &self,
        task: TaskId,
        name: &str,
        bytes: &[u8],
        max_per_task: u32,
        call_id: &str,
    ) -> Result<super::ArtifactMeta, WriteArtifactError> {
        let limits = ArtifactsConfig::default();
        let sniffed = check_artifact(name, bytes, &limits).unwrap();
        write_artifact(
            &self.pool,
            NewArtifact {
                task_id: task,
                run_id: self.run(task).await,
                call_id,
                name,
                sniffed: &sniffed,
                content: bytes,
                max_per_task,
            },
        )
        .await
    }

    /// The artifact-written payloads of the task's transcript, oldest first.
    async fn written_events(&self, task: TaskId) -> Vec<serde_json::Value> {
        sqlx::query_scalar(
            "SELECT payload #> '{factoryActivity,artifactWritten}' FROM transcript_events
             WHERE task_id = $1 AND kind = 'factory_activity' ORDER BY id",
        )
        .bind(task.as_uuid())
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }
}

fn png() -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    RgbImage::new(3, 2)
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

#[tokio::test]
async fn replacing_a_text_artifact_by_an_image_drops_the_text_and_records_the_image_event() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "result.md", b"# text", 50)
        .await
        .unwrap();
    let meta = env.write(task.id, "result.md", &png(), 50).await.unwrap();

    assert_eq!(meta.kind, "image");
    assert_eq!((meta.width, meta.height), (Some(3), Some(2)));
    let all = list_artifacts(&env.pool, task.id).await.unwrap();
    assert_eq!(all.len(), 1);
    let stored = read_artifact_content(&env.pool, task.id, "result.md")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.content, png());
    assert_eq!(stored.meta.mime_type, "image/png");
    let events = env.written_events(task.id).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "ARTIFACT_KIND_TEXT");
    assert_eq!(events[1]["kind"], "ARTIFACT_KIND_IMAGE");
    assert_eq!(events[1]["mimeType"], "image/png");
    assert!(
        read_artifact_content(&env.pool, task.id, "other.md")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_new_name_over_the_count_limit_is_rejected_but_a_replacement_is_not() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    for name in ["a.md", "b.md"] {
        env.write(task.id, name, b"x", 2).await.unwrap();
    }

    let err = env.write(task.id, "c.md", b"x", 2).await.unwrap_err();
    assert!(matches!(
        err,
        WriteArtifactError::Rejected(ArtifactError::TooMany(2))
    ));
    assert!(err.to_string().contains("replace"));
    env.write(task.id, "a.md", b"new", 2).await.unwrap();
    let a = read_artifact_content(&env.pool, task.id, "a.md")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.content, b"new");
}

#[tokio::test]
async fn concurrent_writes_at_49_artifacts_let_exactly_one_succeed() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    for i in 0..49 {
        env.write(task.id, &format!("f{i}.md"), b"x", 50)
            .await
            .unwrap();
    }

    let (a, b) = tokio::join!(
        env.write(task.id, "new-a.md", b"x", 50),
        env.write(task.id, "new-b.md", b"x", 50)
    );

    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(list_artifacts(&env.pool, task.id).await.unwrap().len(), 50);
}

#[tokio::test]
async fn artifacts_of_a_cancelled_task_stay_readable() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "findings.md", b"x", 50).await.unwrap();
    cancel_task(&env.pool, task.id, human()).await.unwrap();

    assert_eq!(list_artifacts(&env.pool, task.id).await.unwrap().len(), 1);
    let stored = read_artifact_content(&env.pool, task.id, "findings.md")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.content, b"x");
}

#[tokio::test]
async fn artifacts_are_listed_newest_update_first() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "a.md", b"x", 50).await.unwrap();
    env.write(task.id, "b.md", b"x", 50).await.unwrap();
    env.write(task.id, "a.md", b"y", 50).await.unwrap();

    let names: Vec<_> = list_artifacts(&env.pool, task.id)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.name)
        .collect();
    assert_eq!(names, ["a.md", "b.md"]);
}

async fn readable(env: &Env, reader: &TaskRow, key: &str) -> bool {
    readable_task(&env.pool, reader.id, key)
        .await
        .unwrap()
        .is_some()
}

#[tokio::test]
async fn related_tasks_are_own_ancestors_descendants_and_dependencies() {
    let env = env().await;
    // DF-1 is a chat-like root; DF-2 its child, DF-3 the grandchild, DF-4 a sibling of DF-3.
    let root = env.task(None, &[]).await;
    let child = env.task(Some(root.id), &[]).await;
    let grandchild = env.task(Some(child.id), &[]).await;
    env.task(Some(child.id), &[]).await; // DF-4, a sibling of DF-3
    let reviewer = env.task(None, &["DF-3"]).await;
    let stranger = env.task(None, &[]).await;

    assert!(readable(&env, &grandchild, "DF-3").await, "own task");
    assert!(readable(&env, &child, "DF-3").await, "child");
    assert!(readable(&env, &root, "DF-3").await, "grandchild");
    assert!(readable(&env, &grandchild, "DF-1").await, "three levels up");
    assert!(readable(&env, &grandchild, "DF-2").await, "parent");
    assert!(readable(&env, &reviewer, "DF-3").await, "dependency");
    assert!(!readable(&env, &grandchild, "DF-5").await, "dependent");
    assert!(!readable(&env, &grandchild, "DF-4").await, "sibling");
    assert!(!readable(&env, &stranger, "DF-1").await, "unrelated");
    // An unknown key looks like an unrelated task.
    assert_eq!(
        readable_task(&env.pool, stranger.id, "DF-999")
            .await
            .unwrap(),
        readable_task(&env.pool, stranger.id, "DF-1").await.unwrap()
    );
}

#[tokio::test]
async fn the_child_ended_message_lists_the_artifacts_of_the_child_only() {
    let env = env().await;
    let parent = env.task(None, &[]).await;
    let child = env.task(Some(parent.id), &[]).await;
    let grandchild = env.task(Some(child.id), &[]).await;
    env.write(child.id, "findings.md", b"x", 50).await.unwrap();
    env.write(grandchild.id, "raw.md", b"x", 50).await.unwrap();
    let silent = env.task(Some(parent.id), &[]).await;

    cancel_task(&env.pool, child.id, human()).await.unwrap();
    cancel_task(&env.pool, silent.id, human()).await.unwrap();

    let messages = child_ended_messages(&env.pool, parent.id).await;
    assert_eq!(messages.len(), 2, "{messages:?}");
    let with = messages.iter().find(|m| m.contains("DF-2 ")).unwrap();
    assert!(
        with.contains("Artifacts: DF-2/findings.md\n")
            || with.ends_with("Artifacts: DF-2/findings.md"),
        "{with}"
    );
    assert!(!with.contains("raw.md"), "{with}");
    let without = messages.iter().find(|m| m.contains("DF-4 ")).unwrap();
    assert!(without.contains("Artifacts: none"), "{without}");
}

#[tokio::test]
async fn a_stored_artifact_appends_one_written_event_to_the_runs_transcript() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "findings.md", b"# Findings", 50)
        .await
        .unwrap();

    let events = env.written_events(task.id).await;

    assert_eq!(
        events,
        vec![serde_json::json!({
            "name": "findings.md", "kind": "ARTIFACT_KIND_TEXT", "mimeType": "text/markdown", "size": "10"
        })]
    );
}

#[tokio::test]
async fn replacing_an_artifact_appends_a_second_event_with_the_new_size() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "findings.md", b"# Findings", 50)
        .await
        .unwrap();
    env.write(task.id, "findings.md", b"# Findings 2", 50)
        .await
        .unwrap();

    let events = env.written_events(task.id).await;

    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["size"], "12");
}

#[tokio::test]
async fn the_same_call_id_stores_one_event() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    let call = uuid::Uuid::now_v7().to_string();
    env.write_call(task.id, "a.md", b"x", 50, &call)
        .await
        .unwrap();
    env.write_call(task.id, "a.md", b"x", 50, &call)
        .await
        .unwrap();

    assert_eq!(env.written_events(task.id).await.len(), 1);
}

#[tokio::test]
async fn a_write_rejected_by_the_count_limit_stores_no_event() {
    let env = env().await;
    let task = env.task(None, &[]).await;
    env.write(task.id, "a.md", b"x", 1).await.unwrap();
    env.write(task.id, "b.md", b"x", 1).await.unwrap_err();

    assert_eq!(env.written_events(task.id).await.len(), 1);
}
