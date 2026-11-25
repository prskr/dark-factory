//! Commit signing (`persona-commit-signing`, `just e2e-local`): the commits of a run are signed by
//! the control plane with the persona's key, and a commit made with signing switched off is
//! rejected by the Git proxy. Needs the environment of `E2eEnv`, so the tests are ignored by
//! default.

use std::sync::Arc;

use anyhow::{Context, Result};
use df_core::TaskStatus;
use df_e2e::fixture::{self, World};
use df_e2e::forgejo::Forgejo;
use df_e2e::wait::{RUN_TIMEOUT, eventually};
use df_e2e::{data, db};

/// Scenarios "Harness commit", "Signed by the persona" and "Forgejo instance": the bot account
/// registers the persona's key, the key is verified on Forgejo with the signature of "Verify key
/// on Forgejo" (Forgejo's settings form, within the token's 1 minute), the demo script commits
/// through git, the shim and the control plane, the push passes `df:signature`, and Forgejo shows
/// the commit as verified.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_persona_commit_shows_as_verified_on_forgejo_once_the_key_is_verified() -> Result<()> {
    fixture::run(
        "a_persona_commit_shows_as_verified_on_forgejo_once_the_key_is_verified",
        |world| async move {
            let audited_before = db::persona_audit_entries(
                &world.pool,
                data::PERSONA_HANDLE,
                "persona.signing_key.forge_verify",
            )
            .await?;
            let issue = world
                .forgejo
                .create_issue(&world.unique("Signed greeting"), "Greet the user.")
                .await?;
            let key = world
                .session
                .create_task(
                    &world.env,
                    data::PROJECT_KEY,
                    "Add a greeting file",
                    Some(issue),
                )
                .await?;
            world.wait_for_status(&key, TaskStatus::Waiting).await?;
            let branch = format!("factory/task/{key}");

            // The persona has its key once its first run started; Forgejo checks a commit's
            // signature when it is shown, so the key may be verified after the push.
            let public_key = db::persona_signing_key(&world.pool, data::PERSONA_HANDLE)
                .await?
                .context("the signing key of the persona")?;
            verify_key_on_forgejo(&world, &public_key).await?;

            // The push passed `df:signature`, so the commit carries a valid signature of the
            // persona; Forgejo knows the verified key and shows the commit as verified.
            let commits = world.forgejo.commits(&branch).await?;
            let commit = greeting(&commits).context("the greeting commit on the forge")?;
            let verification = &commit["commit"]["verification"];
            let signature = verification["signature"].as_str().context("a signature")?;
            let payload = verification["payload"].as_str().context("a payload")?;
            df_core::signing_key::verify(&public_key, payload.as_bytes(), signature.as_bytes())
                .map_err(|e| anyhow::anyhow!("the signature does not verify: {e}"))?;
            assert_eq!(commit["commit"]["committer"]["email"], data::PERSONA_EMAIL);
            assert_eq!(verification["verified"], true, "{verification}");

            // The signature of the token this test had made is audited (entries of earlier runs
            // do not count).
            let audited = db::persona_audit_entries(
                &world.pool,
                data::PERSONA_HANDLE,
                "persona.signing_key.forge_verify",
            )
            .await?;
            assert!(
                audited > audited_before,
                "no new forge_verify audit entry ({audited_before} before, {audited} after)"
            );

            // Every signature is audited with the run and the key's fingerprint.
            let signed = db::run_audit_entries(&world.pool, &key, "commit.sign").await?;
            assert!(signed > 0, "no commit.sign audit entry for {key}");
            Ok(())
        },
    )
    .await
}

/// Registers `public_key` as a new key of the bot account and verifies it: the token comes from
/// Forgejo's API, the signature from the persona dialog "Verify key on Forgejo" (with Forgejo's
/// domain as namespace), the form POST from the bot's own Forgejo session. The token is valid for
/// 1 minute, so it is fetched last, after the sign-in.
async fn verify_key_on_forgejo(world: &World, public_key: &str) -> Result<()> {
    let bot = Forgejo::with_password(
        &world.env.forgejo_url,
        data::FORGE_OWNER,
        data::FORGE_OWNER_PASSWORD,
    )?;
    bot.replace_ssh_key(&world.unique("persona-key"), public_key)
        .await?;
    let web_url = data::forgejo_web_url();
    world
        .session
        .forgejo_login(&web_url, data::FORGE_OWNER, data::FORGE_OWNER_PASSWORD)
        .await?;
    let token = bot.key_verification_token().await?;
    let signature = world
        .session
        .sign_forgejo_token(
            &world.env,
            data::PERSONA_NAME,
            &data::forgejo_domain(),
            &token,
        )
        .await?;
    let fingerprint = df_core::signing_key::fingerprint_of(public_key)?;
    world
        .session
        .forgejo_verify_key(&web_url, &fingerprint, &signature)
        .await
}

/// The commit "Add greeting" of the demo script.
fn greeting(commits: &[serde_json::Value]) -> Option<&serde_json::Value> {
    commits.iter().find(|c| {
        c.pointer("/commit/message")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|m| m.contains("Add greeting"))
    })
}

/// Scenarios "Unsigned commit" and "Re-sign hint": the agent commits with
/// `git -c commit.gpgSign=false`, every push is rejected with `df:signature`, and the agent is
/// told how to sign its commits again. Nothing reaches the forge.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn an_unsigned_commit_is_rejected_with_df_signature_and_the_agent_is_told_how_to_re_sign()
-> Result<()> {
    fixture::run(
        "an_unsigned_commit_is_rejected_with_df_signature_and_the_agent_is_told_how_to_re_sign",
        unsigned_body,
    )
    .await
}

async fn unsigned_body(world: Arc<World>) -> Result<()> {
    let key = world
        .session
        .create_task(
            &world.env,
            data::UNSIGNED_PROJECT_KEY,
            "Commit without a signature",
            None,
        )
        .await?;
    let branch = format!("factory/task/{key}");

    let rejected = eventually("a push rejected for its signature", RUN_TIMEOUT, || async {
        let pushes = db::pushes(&world.pool, &key).await?;
        Ok(pushes
            .into_iter()
            .flat_map(|p| p.refs)
            .find(|r| r.reason.starts_with("df:signature")))
    })
    .await?;
    assert!(
        rejected.reason.contains("missing signature"),
        "{rejected:?}"
    );
    let messages = eventually("the rejection message", RUN_TIMEOUT, || async {
        let messages = db::push_rejected_messages(&world.pool, &key).await?;
        Ok((!messages.is_empty()).then_some(messages))
    })
    .await?;
    assert!(
        messages.iter().all(|m| m.contains("df:signature")
            && m.contains("git rebase --exec 'git commit --amend --no-edit' <base>")),
        "{messages:?}"
    );
    // A signature cannot be approved: no request, and nothing was accepted or reached the forge.
    assert!(db::exception_requests(&world.pool, &key).await?.is_empty());
    assert!(
        db::pushes(&world.pool, &key)
            .await?
            .iter()
            .all(|p| p.verdict != "accepted")
    );
    assert!(!world.forgejo.branch_exists(&branch).await?);
    Ok(())
}
