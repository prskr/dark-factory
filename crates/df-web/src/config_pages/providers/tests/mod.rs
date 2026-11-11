//! Tests of the model provider forms and the settings page: shared fixtures; the tests are
//! grouped by topic in the submodules.

use crate::testkit::{TestApp, form};

mod lifecycle;
mod models;
mod page;
mod personas;
mod scheme;

/// The form of an Anthropic provider; `extra` overrides or adds fields.
pub(super) fn provider_form(credential: &str, extra: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("name", "Anthropic work"),
        ("credential_ref_id", credential),
        ("scheme", "x-api-key"),
        ("api_anthropic_messages", "on"),
        ("url_anthropic_messages", "https://api.anthropic.com"),
    ];
    for (name, value) in extra {
        pairs.retain(|(existing, _)| existing != name);
        pairs.push((name, value));
    }
    form(&pairs)
}

pub(super) async fn credential(app: &TestApp, name: &str) -> String {
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, 'llm-secret', 'api-key')")
        .bind(id)
        .bind(name)
        .execute(&app.pool)
        .await
        .unwrap();
    id.to_string()
}

pub(super) async fn stored(app: &TestApp, name: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM model_providers WHERE name = $1")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}
