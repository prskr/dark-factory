//! Tests of the chat read models (container-backed).

use df_core::{PersonaId, Principal, ProjectId, ProjectKey, QuestionSpec, TaskId, UserId};
use serde_json::json;
use sqlx::PgPool;

use super::*;
use crate::chats::{NewChat, archive_chat, create_chat};
use crate::requests::{Raise, raise_question};
use crate::task_events::set_status_for_test;
use crate::tasks::tests::seed_project_and_persona;

struct Fixture {
    pool: PgPool,
    project_id: ProjectId,
    persona_id: PersonaId,
}

impl Fixture {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let key: ProjectKey = "DF".parse().unwrap();
        let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Grace")
            .await
            .unwrap();
        Self {
            pool,
            project_id,
            persona_id,
        }
    }

    async fn chat(&self, text: &str) -> TaskId {
        let (chat, _) = create_chat(
            &self.pool,
            NewChat {
                project_id: self.project_id,
                persona_id: self.persona_id,
                author: Principal::human(UserId::new(), "Ada"),
                text: text.into(),
            },
        )
        .await
        .unwrap();
        set_status_for_test(&self.pool, chat.id, TaskStatus::Waiting).await;
        chat.id
    }

    async fn event(&self, chat: TaskId, kind: &str, payload: serde_json::Value) {
        let run: Option<Uuid> = sqlx::query_scalar("SELECT id FROM runs WHERE task_id = $1")
            .bind(chat.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .unwrap();
        let run = if let Some(run) = run {
            run
        } else {
            let run = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'succeeded')",
            )
            .bind(run)
            .bind(chat.as_uuid())
            .execute(&self.pool)
            .await
            .unwrap();
            run
        };
        sqlx::query(
            "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, (SELECT COALESCE(max(seq), 0) + 1 FROM transcript_events
                              WHERE run_id = $2), now(), $3, $4)",
        )
        .bind(chat.as_uuid())
        .bind(run)
        .bind(kind)
        .bind(payload)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn agent_says(&self, chat: TaskId, message_id: &str, chunks: &[&str]) {
        for chunk in chunks {
            self.event(
                chat,
                KIND_AGENT_MESSAGE_CHUNK,
                json!({"agentMessageChunk": {"messageId": message_id,
                    "content": {"text": {"text": chunk}}}}),
            )
            .await;
        }
    }

    async fn user_says(&self, chat: TaskId, text: &str) {
        self.event(
            chat,
            KIND_USER_MESSAGE,
            json!({"userMessage": {"content": [
                {"text": {"text": text}, "hidden": false},
                {"text": {"text": "call set_task_title"}, "hidden": true}]}}),
        )
        .await;
    }

    async fn ask(&self, chat: TaskId) {
        let agent = Principal::system("test");
        let from = Raise {
            task_id: chat,
            run_id: None,
            actor: &agent,
            blocking: false,
        };
        raise_question(&self.pool, from, QuestionSpec::text_question("Which one?"))
            .await
            .unwrap();
    }

    async fn keys(&self, archived: bool, search: Option<&str>) -> Vec<String> {
        list_chats(&self.pool, "DF", archived, search)
            .await
            .unwrap()
            .into_iter()
            .map(|chat| chat.key)
            .collect()
    }
}

#[tokio::test]
async fn chat_views_list_a_chat_that_needs_you_first_and_the_others_by_last_activity() {
    let f = Fixture::new().await;
    let quiet = f.chat("First").await;
    let needs = f.chat("Second").await;
    let recent = f.chat("Third").await;
    f.ask(needs).await;
    f.user_says(recent, "Latest word").await;
    f.user_says(quiet, "Old word").await;
    // `quiet` was touched last, so by activity alone it would be first.
    f.user_says(quiet, "Newest word").await;

    let listed = list_chats(&f.pool, "DF", false, None).await.unwrap();

    let order: Vec<_> = listed
        .iter()
        .map(|c| (c.key.as_str(), c.needs_you))
        .collect();
    assert_eq!(
        order,
        [
            ("DF-2-CHAT", true),
            ("DF-1-CHAT", false),
            ("DF-3-CHAT", false)
        ]
    );
}

#[tokio::test]
async fn chat_views_show_the_last_visible_message_of_the_board_member_or_the_agent() {
    let f = Fixture::new().await;
    let human = f.chat("Human").await;
    let agent = f.chat("Agent").await;
    f.user_says(human, "Plan the release").await;
    f.user_says(agent, "Plan the release").await;
    f.agent_says(agent, "m1", &["Here is ", "the plan."]).await;
    f.agent_says(agent, "m2", &["Second ", "message."]).await;

    let listed = list_chats(&f.pool, "DF", false, None).await.unwrap();

    let message = |key: &str| {
        listed
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.last_message.clone())
            .unwrap()
    };
    assert_eq!(message("DF-1-CHAT"), "Plan the release");
    assert_eq!(message("DF-2-CHAT"), "Second message.");
}

#[tokio::test]
async fn chat_views_mark_a_project_with_a_chat_that_needs_you_and_count_open_chats() {
    let f = Fixture::new().await;
    let needs = f.chat("One").await;
    let archived = f.chat("Two").await;
    f.chat("Three").await;
    f.ask(needs).await;
    archive_chat(&f.pool, archived, Principal::human(UserId::new(), "Ada"))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES (gen_random_uuid(), 'DOC', 'Docs', 'https://g/d.git', 'img', 'main')",
    )
    .execute(&f.pool)
    .await
    .unwrap();

    let projects = list_chat_projects(&f.pool).await.unwrap();

    let facts: Vec<_> = projects
        .iter()
        .map(|p| (p.key.as_str(), p.chats, p.archived, p.needs_you))
        .collect();
    // By name: "Docs" before "Project DF".
    assert_eq!(facts, [("DOC", 0, 0, false), ("DF", 2, 1, true)]);
}

#[tokio::test]
async fn chat_views_keep_archived_chats_out_of_the_list_and_search_them() {
    let f = Fixture::new().await;
    let topic = f.chat("Plan the Helm chart release").await;
    let agent_text = f.chat("Something else").await;
    let user_text = f.chat("Third topic").await;
    f.chat("Helm chart, still open").await;
    let human = Principal::human(UserId::new(), "Ada");
    for chat in [topic, agent_text, user_text] {
        archive_chat(&f.pool, chat, human.clone()).await.unwrap();
    }
    f.agent_says(agent_text, "m1", &["Run the he", "lm upgrade."])
        .await;
    f.user_says(user_text, "Mind the HELM values").await;

    assert_eq!(f.keys(false, None).await, ["DF-4-CHAT"]);
    assert_eq!(
        f.keys(true, None).await,
        ["DF-3-CHAT", "DF-2-CHAT", "DF-1-CHAT"]
    );
    // Topic, agent text split over chunks and text of the board member all count.
    let found = f.keys(true, Some("helm")).await;
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(f.keys(true, Some("upgrade")).await, ["DF-2-CHAT"]);
    assert_eq!(f.keys(true, Some("GRACE")).await.len(), 3, "persona");
    assert!(
        f.keys(true, Some("100%")).await.is_empty(),
        "wildcards are literal"
    );
}

#[tokio::test]
async fn chat_views_find_the_project_of_the_chat_active_last() {
    let f = Fixture::new().await;
    assert_eq!(latest_chat_project(&f.pool).await.unwrap(), None);
    f.chat("Hello").await;
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES (gen_random_uuid(), 'WEB', 'Web', 'https://g/w.git', 'img', 'main')",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let web: Uuid = sqlx::query_scalar("SELECT id FROM projects WHERE key = 'WEB'")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    create_chat(
        &f.pool,
        NewChat {
            project_id: ProjectId::from_uuid(web),
            persona_id: f.persona_id,
            author: Principal::human(UserId::new(), "Ada"),
            text: "Later".into(),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        latest_chat_project(&f.pool).await.unwrap().as_deref(),
        Some("WEB")
    );
}

#[test]
fn like_patterns_escape_wildcards() {
    assert_eq!(like_pattern("100%_a\\b"), "%100\\%\\_a\\\\b%");
}
