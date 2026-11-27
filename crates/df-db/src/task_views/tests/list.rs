//! The task list, task details, runs and the transcript.

use super::*;

#[tokio::test]
async fn lists_tasks_details_runs_usage_and_transcript() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, run) = seed(&pool).await;

    let list = list_task_items(&pool, &TaskFilter::default())
        .await
        .unwrap();
    assert_eq!(
        (list[0].key.to_string().as_str(), list[0].status.as_str()),
        ("DF-1", "queued")
    );
    let detail = get_task_detail_by_key(&pool, "DF-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (detail.project_key.as_str(), detail.issue_number),
        ("DF", Some(7))
    );
    assert_eq!(
        get_task_detail(&pool, task).await.unwrap().unwrap().id,
        detail.id
    );
    assert!(
        get_task_detail_by_key(&pool, "DF-9")
            .await
            .unwrap()
            .is_none()
    );

    let ended = || serde_json::json!({"turnEnded": {}});
    let first = insert_event(&pool, task, run, 1, "turn_ended", ended()).await;
    insert_event(&pool, task, run, 2, "turn_ended", ended()).await;
    insert_event(&pool, task, run, 3, "turn_ended", ended()).await;
    for (input, model) in [(10, "opus"), (20, "opus"), (1, "haiku")] {
        insert_usage(&pool, run, Some(model), input, 5, 2).await;
    }
    insert_usage(&pool, run, None, 0, 0, 0).await;
    insert_usage(&pool, run, None, 0, 0, 0).await;
    let last = insert_event(&pool, task, run, 4, "bogus", serde_json::json!({"nope": 1})).await;

    let runs = list_runs(&pool, task).await.unwrap();
    assert_eq!(runs.len(), 1);
    let opus = runs[0].usage.iter().find(|u| u.model_id == "opus").unwrap();
    assert_eq!(
        (
            opus.input_tokens,
            opus.output_tokens,
            opus.cache_read_tokens,
            opus.cache_write_tokens
        ),
        (30, 10, 4, 0)
    );
    assert_eq!(runs[0].usage.len(), 2);
    assert_eq!(runs[0].unknown_usage_requests, 2);

    let all = transcript_after(&pool, task, 0, 100).await.unwrap();
    assert_eq!(all.len(), 4);
    assert!(all[0].event.is_some());
    assert!(all[3].event.is_none());
    let tail = transcript_after(&pool, task, first, 100).await.unwrap();
    assert_eq!(tail.first().map(|e| e.id), Some(first + 1));
    assert_eq!(tail.last().map(|e| e.id), Some(last));
    assert_eq!(transcript_after(&pool, task, 0, 2).await.unwrap().len(), 2);
}

#[tokio::test]
async fn turn_ends_stored_with_the_removed_usage_fields_still_decode() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, run) = seed(&pool).await;
    let old = serde_json::json!({
        "schemaVersion": 1,
        "turnEnded": {
            "stopReason": "STOP_REASON_END_TURN",
            "usage": [{"modelId": "opus", "inputTokens": "10"}],
            "usageSource": 1
        }
    });
    insert_event(&pool, task, run, 1, "turn_ended", old).await;

    let rows = transcript_after(&pool, task, 0, 10).await.unwrap();

    let Some(Event {
        kind: Some(df_proto::transcript::v1::event::Kind::TurnEnded(ended)),
        ..
    }) = &rows[0].event
    else {
        panic!("the old turn end does not decode: {:?}", rows[0].event);
    };
    assert_eq!(
        ended.stop_reason(),
        df_proto::transcript::v1::StopReason::EndTurn
    );
}

#[tokio::test]
async fn lists_runs_newest_first() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, _) = seed(&pool).await;
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 2, 'succeeded')")
        .bind(Uuid::now_v7())
        .bind(task.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let numbers: Vec<i64> = list_runs(&pool, task)
        .await
        .unwrap()
        .iter()
        .map(|r| r.number)
        .collect();
    assert_eq!(numbers, [2, 1]);
}

/// Three tasks with chosen times: DF-1 updated last, DF-2 and DF-3 with the same update time,
/// and a creation order (DF-3, DF-1, DF-2) unlike the order of the IDs, so that the sorts
/// and the tie breaker (ID) can be told apart.
async fn seed_sorted_tasks(pool: &PgPool) {
    seed(pool).await;
    let (project, persona): (Uuid, Uuid) =
        sqlx::query_as("SELECT project_id, persona_id FROM tasks")
            .fetch_one(pool)
            .await
            .unwrap();
    for title in ["Second", "Third"] {
        crate::create_task(
            pool,
            crate::NewTask {
                project_id: df_core::ProjectId::from_uuid(project),
                persona_id: df_core::PersonaId::from_uuid(persona),
                title: Some(title.into()),
                description: String::new(),
                issue_number: None,
                created_by: Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
    }
    for (key, created, updated) in [("DF-1", 10, 50), ("DF-2", 20, 20), ("DF-3", 0, 20)] {
        sqlx::query(
            "UPDATE tasks SET created_at = '2026-11-01T00:00:00Z'::timestamptz + make_interval(mins => $2::int),
                              updated_at = '2026-11-01T00:00:00Z'::timestamptz + make_interval(mins => $3::int)
             WHERE key = $1",
        )
        .bind(key)
        .bind(created)
        .bind(updated)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn lists_tasks_in_each_sort_with_a_stable_tie_order() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    seed_sorted_tasks(&pool).await;
    let keys = |sort| {
        let pool = pool.clone();
        async move {
            list_task_items(
                &pool,
                &TaskFilter {
                    sort,
                    ..TaskFilter::default()
                },
            )
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.key.to_string())
            .collect::<Vec<_>>()
        }
    };

    // The default is the last update, newest first.
    assert_eq!(TaskFilter::default().sort, TaskSort::Updated);
    // Every variant, so a sort whose key the query does not know cannot pass: it would fall
    // through to the tie breaker alone (newest ID first).
    for sort in TaskSort::ALL {
        let expected = match sort {
            // DF-2 and DF-3 tie on the update time; the tie breaks by ID, newest first.
            TaskSort::Updated => ["DF-1", "DF-3", "DF-2"],
            TaskSort::UpdatedAsc => ["DF-2", "DF-3", "DF-1"],
            TaskSort::Created => ["DF-2", "DF-1", "DF-3"],
        };
        assert_eq!(keys(sort).await, expected, "{sort:?}");
    }
}

#[tokio::test]
async fn the_sort_keeps_the_status_and_project_filters() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    seed_sorted_tasks(&pool).await;
    let filter = TaskFilter {
        group: Some(StatusSet::Active),
        project_key: Some("DF".into()),
        scope: None,
        sort: TaskSort::UpdatedAsc,
    };
    let keys: Vec<_> = list_task_items(&pool, &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.key.to_string())
        .collect();
    assert_eq!(keys, ["DF-2", "DF-3", "DF-1"]);
}

#[tokio::test]
async fn filters_tasks_by_status_group_and_project_and_counts_per_project() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, _) = seed(&pool).await;
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'WEB', 'Website', 'https://g/o/w.git', 'img')")
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap();
    let by = |group, project: Option<&str>| TaskFilter {
        group,
        project_key: project.map(str::to_string),
        ..TaskFilter::default()
    };

    let active = list_task_items(&pool, &by(Some(StatusSet::Active), None))
        .await
        .unwrap();
    let done = list_task_items(&pool, &by(Some(StatusSet::Done), None))
        .await
        .unwrap();
    let web = list_task_items(&pool, &by(None, Some("WEB")))
        .await
        .unwrap();
    let df = list_task_items(&pool, &by(None, Some("DF"))).await.unwrap();
    assert_eq!(
        (active.len(), done.len(), web.len(), df.len()),
        (1, 0, 0, 1)
    );

    crate::task_events::set_status_for_test(&pool, task, TaskStatus::Completed).await;
    let active = list_task_items(&pool, &by(Some(StatusSet::Active), None))
        .await
        .unwrap();
    let done = list_task_items(&pool, &by(Some(StatusSet::Done), None))
        .await
        .unwrap();
    assert_eq!((active.len(), done.len()), (0, 1));

    let counts = list_project_filter_items(&pool, Some(StatusSet::Done))
        .await
        .unwrap();
    let counts: Vec<_> = counts
        .iter()
        .map(|p| (p.key.as_str(), p.task_count))
        .collect();
    assert_eq!(counts, [("DF", 1), ("WEB", 0)]);
    let all = list_project_filter_items(&pool, None).await.unwrap();
    assert_eq!(all[0].task_count, 1);
}

#[tokio::test]
async fn narrower_sets_and_counts_per_project_and_status() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, _) = seed(&pool).await;
    let by = |set| TaskFilter {
        group: Some(set),
        project_key: None,
        ..TaskFilter::default()
    };
    crate::task_events::set_status_for_test(&pool, task, TaskStatus::RateLimited).await;
    let running = list_task_items(&pool, &by(StatusSet::Running))
        .await
        .unwrap();
    let queued = list_task_items(&pool, &by(StatusSet::Queued))
        .await
        .unwrap();
    assert_eq!((running.len(), queued.len()), (1, 0));

    let counts = project_status_counts(&pool).await.unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].status, "rate_limited");
    assert_eq!(counts[0].count, 1);
}
