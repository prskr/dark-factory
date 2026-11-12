//! Landlock isolation of the harness (`just e2e-local`): the fake agent of project `LL` cannot
//! read the runtime's private key and cannot write `/opt/probe-writable` (owned by the sandbox
//! user, so only Landlock denies the write), but a factory tool call still reaches the control
//! plane. Needs the environment of `E2eEnv`, so the test is ignored by default.
//!
//! `just e2e-local` sets `E2E_LANDLOCK=disabled` when the kernel of the cluster offers no Landlock
//! ABI 6; the check is skipped then and says why.

use anyhow::Result;
use df_core::TaskId;
use df_db::requests::RequestKind;
use df_e2e::browser::Session;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{E2eEnv, data, db};
use uuid::Uuid;

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_harness_is_denied_the_private_key_and_the_system_but_keeps_its_factory_tools()
-> Result<()> {
    if std::env::var("E2E_LANDLOCK").as_deref() == Ok("disabled") {
        eprintln!(
            "skipping the Landlock check: the cluster kernel has no Landlock ABI 6 (Linux 6.12 \
             or newer with `landlock` in /sys/kernel/security/lsm), so the suite runs with \
             sandbox.landlock: disabled"
        );
        return Ok(());
    }
    let env = E2eEnv::from_env()?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;
    let pool = db::connect(&env.database_url).await?;

    let key = session
        .create_task(&env, data::LANDLOCK_PROJECT_KEY, "Probe the sandbox", None)
        .await?;

    for text in [data::LANDLOCK_KEY_DENIED, data::LANDLOCK_WRITE_DENIED] {
        eventually(
            &format!("transcript event containing `{text}`"),
            RUN_TIMEOUT,
            || async {
                let events = db::transcript_events_containing(&pool, &key, text).await?;
                Ok((events >= 1).then_some(()))
            },
        )
        .await?;
    }
    // The factory tool call reached the control plane through the runtime socket.
    let task_id = TaskId::from_uuid(
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM tasks WHERE key = $1")
            .bind(&key)
            .fetch_one(&pool)
            .await?,
    );
    eventually(
        "the question of the factory tool call",
        RUN_TIMEOUT,
        || async {
            Ok(df_db::requests::list_for_task(&pool, task_id)
                .await?
                .into_iter()
                .find(|r| {
                    r.kind == RequestKind::Question && r.question_text() == data::LANDLOCK_QUESTION
                })
                .map(|_| ()))
        },
    )
    .await?;
    Ok(())
}
