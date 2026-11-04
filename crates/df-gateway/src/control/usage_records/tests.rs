use df_agent_channel::metrics::probe::Probe;

use super::*;

/// Records into the agent channel's metrics like the binary does.
struct Counter(df_agent_channel::Metrics);

impl TokenCounter for Counter {
    fn record_tokens(&self, model: &str, counts: [(&'static str, u64); 4]) {
        self.0.record_tokens(model, counts);
    }
}

/// A run (any state) to attach usage to.
async fn run(pool: &PgPool) -> Uuid {
    let (project, persona, task, run) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let (credential, provider) = (Uuid::now_v7(), Uuid::now_v7());
    for (query, id) in [
        (
            "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'DF', 'P', 'https://g/o/r.git', 'img')",
            project,
        ),
        (
            "INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, 'c', 's', 'k')",
            credential,
        ),
    ] {
        sqlx::query(query).bind(id).execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme) VALUES ($1, 'p', $2, 'bearer')")
        .bind(provider)
        .bind(credential)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO personas (id, name, system_prompt, model_provider_id) VALUES ($1, 'p', 'x', $2)")
        .bind(persona)
        .bind(provider)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id) VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3)")
        .bind(task)
        .bind(project)
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(run)
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
    run
}

fn message(run: Uuid, model: &str, input: u64, output: u64) -> pb::UsageRecord {
    pb::UsageRecord {
        request_id: Uuid::now_v7().to_string(),
        run_id: run.to_string(),
        at: Some(crate::flow::timestamp_now()),
        model: model.into(),
        wire_api: pb::WireApi::AnthropicMessages.into(),
        upstream_status: 200,
        usage_known: true,
        input_tokens: input,
        output_tokens: output,
        ..pb::UsageRecord::default()
    }
}

async fn report(service: &UsageRecords, records: Vec<pb::UsageRecord>) {
    service
        .report(Request::new(pb::UsageBatch {
            gateway_pod: "gateway-0".into(),
            records,
        }))
        .await
        .unwrap();
}

async fn stored(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM model_usage")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_report_twice_stores_each_request_once_and_counts_its_tokens_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let run = run(&pool).await;
    let probe = Probe::new();
    let service = UsageRecords::new(
        pool.clone(),
        Arc::new(Counter(df_agent_channel::Metrics::new(&probe.meter()))),
    );
    let batch = vec![message(run, "claude-sonnet-5-5", 100, 20)];

    report(&service, batch.clone()).await;
    report(&service, batch).await;

    assert_eq!(stored(&pool).await, 1);
    let model = ("model", "claude-sonnet-5-5");
    assert_eq!(
        probe.total("df.agent.tokens", &[model, ("token_type", "input")]),
        100.0
    );
    assert_eq!(
        probe.total("df.agent.tokens", &[model, ("token_type", "output")]),
        20.0
    );
}

#[tokio::test]
async fn records_of_unknown_runs_and_invalid_records_are_dropped_and_the_rest_is_stored() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let run = run(&pool).await;
    let service = UsageRecords::new(
        pool.clone(),
        Arc::new(Counter(df_agent_channel::Metrics::default())),
    );
    let invalid = pb::UsageRecord {
        request_id: "not-a-uuid".into(),
        ..message(run, "m", 1, 1)
    };

    report(
        &service,
        vec![
            message(Uuid::now_v7(), "m", 1, 1),
            invalid,
            message(run, "m", 1, 1),
        ],
    )
    .await;

    assert_eq!(stored(&pool).await, 1);
}

#[tokio::test]
async fn a_request_without_usage_is_stored_as_unknown_and_counts_no_tokens() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let run = run(&pool).await;
    let probe = Probe::new();
    let service = UsageRecords::new(
        pool.clone(),
        Arc::new(Counter(df_agent_channel::Metrics::new(&probe.meter()))),
    );
    let unknown = pb::UsageRecord {
        usage_known: false,
        upstream_status: 0,
        model: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        ..message(run, "", 0, 0)
    };

    report(&service, vec![unknown]).await;

    let known: bool = sqlx::query_scalar("SELECT usage_known FROM model_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!known);
    assert_eq!(probe.total("df.agent.tokens", &[]), 0.0);
}
