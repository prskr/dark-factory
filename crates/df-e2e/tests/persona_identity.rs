//! Persona identity (`persona-identity`, `just e2e-local`): a persona created through the UI
//! commits under its own identity, and the transcript names it as the author. Needs the environment
//! of `E2eEnv`, so the test is ignored by default.

use anyhow::{Context, Result};
use df_core::TaskStatus;
use df_e2e::browser::button;
use df_e2e::data;
use df_e2e::fixture;
use df_e2e::wait::{RUN_TIMEOUT, UI_TIMEOUT, eventually};
use playwright_rs::expect;
use playwright_rs::protocol::SelectOption;

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_new_persona_commits_under_its_identity_and_authors_the_transcript() -> Result<()> {
    fixture::run(
        "a_new_persona_commits_under_its_identity_and_authors_the_transcript",
        |world| async move {
            let page = &world.session.page;
            let handle = world.unique("ident");
            let display_name = format!("Identity Bot {}", world.suffix);
            let email = format!("{handle}@example.org");

            // The form: avatar preview, then the three identity fields.
            page.goto(&format!("{}/personas", world.env.base_url), None)
                .await?;
            page.locator("button[hx-get='/personas/new']:visible")
                .first()
                .click(None)
                .await?;
            // Scenario "Avatar preview": typing the name changes the preview without a reload.
            let preview = page.locator("#form [role=img][aria-label]");
            page.locator("#display_name")
                .fill("Code Reviewer", None)
                .await?;
            expect(preview).to_contain_text("CR").await?;
            page.locator("#display_name")
                .fill(&display_name, None)
                .await?;
            page.locator("#handle").fill(&handle, None).await?;
            page.locator("#commit_email").fill(&email, None).await?;
            page.locator("#system_prompt")
                .fill("Commit as yourself.", None)
                .await?;
            page.locator("#model_provider_id")
                .select_option(
                    SelectOption::Label(data::MODEL_PROVIDER_NAME.to_string()),
                    None,
                )
                .await?;
            page.locator("#model")
                .select_option(
                    SelectOption::Label(format!("{} (Anthropic Messages)", data::PERSONA_MODEL)),
                    None,
                )
                .await?;
            button(page, "Create persona").click(None).await?;
            world
                .session
                .expect_text(&format!("@{handle}"), UI_TIMEOUT)
                .await?;

            // A task of that persona: the draft PR script commits with the runtime's identity.
            let issue = world
                .forgejo
                .create_issue(&world.unique("Identity greeting"), "Greet the user.")
                .await?;
            let key = world
                .session
                .create_task_for_persona(
                    &world.env,
                    data::PROJECT_KEY,
                    "Add a greeting file",
                    Some(issue),
                    &display_name,
                )
                .await?;
            world
                .session
                .expect_text("Pushed the greeting. Tell me what to change.", RUN_TIMEOUT)
                .await?;
            world.wait_for_status(&key, TaskStatus::Waiting).await?;

            // The commit on the forge: author and committer are the persona.
            let branch = format!("factory/task/{key}");
            let commits = eventually("the commit of the persona", RUN_TIMEOUT, || async {
                Ok(Some(world.forgejo.commits(&branch).await?).filter(|c| !c.is_empty()))
            })
            .await?;
            let greeting = commits
                .iter()
                .find(|c| {
                    c["commit"]["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("Add greeting"))
                })
                .with_context(|| format!("the greeting commit in {commits:?}"))?;
            for role in ["author", "committer"] {
                assert_eq!(greeting["commit"][role]["name"], display_name.as_str());
                assert_eq!(greeting["commit"][role]["email"], email.as_str());
            }

            // The transcript author, and the persona on the task row.
            world
                .session
                .expect_text(&format!("{display_name} @{handle}"), UI_TIMEOUT)
                .await?;
            Ok(())
        },
    )
    .await
}
