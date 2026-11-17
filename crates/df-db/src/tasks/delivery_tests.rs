//! Tests of confirming the delivery of queued prompts (container-backed).

use super::tests::{insert_run, new_task, seed_project_and_persona};
use super::*;

#[tokio::test]
async fn a_prompt_of_another_task_is_not_marked_delivered() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("Mine")))
        .await
        .unwrap();
    let (other, foreign) = create_task(&pool, new_task(project_id, persona_id, Some("Other")))
        .await
        .unwrap();
    let run = insert_run(&pool, task.id).await;

    // The runtime of `task` names the prompt of `other`.
    assert!(
        !mark_prompt_delivered(&pool, task.id, foreign.id, run)
            .await
            .unwrap()
    );
    assert_eq!(
        list_undelivered_messages(&pool, other.id)
            .await
            .unwrap()
            .len(),
        1
    );
}
