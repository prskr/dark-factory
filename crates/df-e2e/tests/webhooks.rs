//! Forge webhooks (`iter3-webhooks`, `just e2e-local`): the e2e Forgejo delivers the events of
//! `factory/demo` to the control plane, and a review by a member wakes a task in review. Needs the
//! environment of `E2eEnv`, so the test is ignored by default.

use std::time::Duration;

use anyhow::{Context, Result};
use df_core::TaskStatus;
use df_e2e::browser::button;
use df_e2e::fixture;
use df_e2e::forgejo::ReviewEvent;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use df_e2e::{data, db};
use playwright_rs::expect;

/// How long reading the clipboard after "Copy secret" may take.
const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn member_review_wakes_the_task_in_review_and_its_pr_is_a_draft_again() -> Result<()> {
    fixture::run(
        "member_review_wakes_the_task_in_review_and_its_pr_is_a_draft_again",
        |world| async move {
            let key = world
                .session
                .create_task(
                    &world.env,
                    data::WEBHOOK_PROJECT_KEY,
                    "Add a welcome file",
                    None,
                )
                .await?;
            let branch = format!("factory/task/{key}");

            // Run 1 completes the task: in review with a ready pull request.
            world.wait_for_status(&key, TaskStatus::InReview).await?;
            let number = eventually("a ready pull request", RUN_TIMEOUT, || async {
                Ok(world
                    .forgejo
                    .pull_of(data::BASE_BRANCH, &branch)
                    .await?
                    .filter(|pr| pr["draft"] == false)
                    .and_then(|pr| pr["number"].as_u64()))
            })
            .await?;

            world
                .reviewer
                .submit_review(
                    number,
                    ReviewEvent::RequestChanges,
                    "Please greet in German as well.",
                )
                .await?;

            // The agent gets the long text; the UI shows the event as one sentence, first in the
            // stack of queued messages and, once delivered, as a row of the transcript.
            let line = format!(
                "{} submitted a review on PR #{number}: changes requested.",
                data::FORGE_REVIEWER
            );
            let sentence = format!("{} requested changes on PR #{number}", data::FORGE_REVIEWER);
            world.session.expect_text(&sentence, UI_TIMEOUT).await?;
            let messages =
                db::system_messages(&world.pool, &key, df_core::system_reason::WEBHOOK).await?;
            assert_eq!(messages.len(), 1, "{messages:?}");
            assert!(messages[0].contains(&line), "{messages:?}");
            // Run 2 of the script reads the feedback and waits; the reopened task's pull request
            // stays a draft.
            eventually(
                "the pull request to be a draft again",
                RUN_TIMEOUT,
                || async {
                    Ok(world
                        .forgejo
                        .pull_of(data::BASE_BRANCH, &branch)
                        .await?
                        .and_then(|pr| (pr["draft"] == true).then_some(())))
                },
            )
            .await?;
            world.wait_for_status(&key, TaskStatus::Waiting).await?;
            world
                .session
                .expect_text("Read the review; waiting for more.", RUN_TIMEOUT)
                .await?;
            // Delivered: the transcript has the row with the link to the pull request, and no
            // bubble with the hidden wake-up text.
            world
                .session
                .expect_region_text("#transcript", &sentence, UI_TIMEOUT)
                .await?;
            // The rows of the pull request itself link to it too: pick the webhook row by its text.
            let link = world.session.page.locator(format!(
                "#transcript [data-system]:has-text('{sentence}') a[href$='/pulls/{number}']"
            ));
            expect(link)
                .with_timeout(UI_TIMEOUT)
                .to_contain_text(&format!("#{number}"))
                .await
                .context("the row links to the pull request")?;
            let transcript = world.session.region_text("#transcript").await?;
            assert!(!transcript.contains(&line), "{transcript}");
            let pull = world
                .forgejo
                .pull_of(data::BASE_BRANCH, &branch)
                .await?
                .context("the pull request of the task")?;
            assert_eq!(pull["draft"], true, "{:?}", pull["title"]);
            assert_eq!(pull["state"], "open");
            Ok(())
        },
    )
    .await
}

/// forge-webhooks, "Incoming webhook URL in project settings" and "Masked webhook secret": the URL of
/// the group is the one the repository webhook of the suite delivers to, and the secret is the one
/// it signs with. Where the page is a secure context (HTTPS or a loopback host) "Copy secret" puts
/// the secret on the clipboard; otherwise the group says that copying needs HTTPS and "Show secret"
/// reveals it.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_integrations_tab_shows_the_webhook_url_and_the_secret_of_the_repository_webhook()
-> Result<()> {
    fixture::run(
        "the_integrations_tab_shows_the_webhook_url_and_the_secret_of_the_repository_webhook",
        |world| async move {
            let session = &world.session;
            session
                .page
                .context()?
                .grant_permissions(&["clipboard-read", "clipboard-write"], None)
                .await
                .context("grant the clipboard permissions")?;
            session
                .page
                .goto(
                    &format!(
                        "{}/projects/{}?tab=integrations",
                        world.env.base_url,
                        data::WEBHOOK_PROJECT_KEY
                    ),
                    None,
                )
                .await?;

            let url = session.page.locator("#webhook-url");
            expect(url.clone())
                .to_have_value(&format!("{}/webhooks/forgejo", world.env.base_url))
                .await
                .context("the webhook URL of the plugin")?;
            // The page holds a mask only.
            let secret_field = session.page.locator("#webhook-secret");
            assert_ne!(secret_field.input_value(None).await?, data::WEBHOOK_SECRET);

            let can_copy = session
                .page
                .evaluate_value("window.isSecureContext && !!navigator.clipboard")
                .await?
                == "true";
            if can_copy {
                button(&session.page, "Copy secret")
                    .click(None)
                    .await
                    .context("copy the secret")?;
                expect(session.page.locator("[role=status]:has-text('Copied')"))
                    .to_be_visible()
                    .await
                    .context("the copy is confirmed")?;
                // A clipboard read can wait for focus indefinitely; bound it so the test fails
                // instead.
                let clipboard = tokio::time::timeout(
                    CLIPBOARD_TIMEOUT,
                    session
                        .page
                        .evaluate_value("navigator.clipboard.readText()"),
                )
                .await
                .context("read the clipboard")??;
                assert_eq!(clipboard, data::WEBHOOK_SECRET);
                assert_ne!(
                    secret_field.input_value(None).await?,
                    data::WEBHOOK_SECRET,
                    "copying does not show it"
                );
            } else {
                expect(session.page.locator("text=Copying needs HTTPS"))
                    .to_be_visible()
                    .await
                    .context("the group says why there is no copy button")?;
            }
            button(&session.page, "Show secret")
                .click(None)
                .await
                .context("show the secret")?;
            expect(secret_field)
                .to_have_value(data::WEBHOOK_SECRET)
                .await
                .context("the revealed secret is the one of the repository webhook")
        },
    )
    .await
}
