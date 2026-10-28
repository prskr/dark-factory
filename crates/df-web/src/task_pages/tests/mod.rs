//! Shared fixtures of the tests; the tests themselves live in the topic modules.

use crate::testkit::TestApp;

mod create;
mod detail;
mod layout;
mod list;
mod messages;

async fn set_status(app: &TestApp, status: &str) {
    sqlx::query("UPDATE tasks SET status = $1")
        .bind(status)
        .execute(&app.pool)
        .await
        .unwrap();
}
