//! Lifecycle events of a project through the web UI (`just e2e-local`): a subscription created on
//! the "Lifecycle events" group of the project settings page's Integrations tab receives a signed `sandbox.provisioned` event when a task of
//! the project starts its first run. The receiver is deploy/e2e/receiver.yaml, reachable over
//! plain http because the e2e control plane is a `dev-tools` build that runs with
//! `--unrestricted-project-subscriptions`. Needs the environment of `E2eEnv` and
//! `E2E_RECEIVER_URL`, so the test is ignored by default.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use df_e2e::browser::Session;
use df_e2e::wait::{self, RUN_TIMEOUT};
use df_e2e::{E2eEnv, data};
use regex::Regex;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use serde_json::Value;

/// The receiver as the control plane reaches it.
const RECEIVER_IN_CLUSTER: &str = "http://receiver.receiver.svc:8080/hook";

const EVENT: &str = "sandbox.provisioned";

/// The content of the "Lifecycle events" group of the Integrations tab.
const REGION: &str = "#lifecycle-events";

const UI_TIMEOUT: Duration = Duration::from_secs(30);

/// One POST the receiver stored.
#[derive(Debug, Deserialize)]
struct Received {
    headers: HashMap<String, String>,
    body: String,
}

#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn project_subscription_receives_a_signed_sandbox_provisioned_event() -> Result<()> {
    let env = E2eEnv::from_env()?;
    let receiver = std::env::var("E2E_RECEIVER_URL").context("E2E_RECEIVER_URL is not set")?;
    let session = Session::start(&env).await?;
    session.login(&env).await?;

    let public_key = subscribe(&session, &env).await?;
    edit_with_the_submit_button(&session).await?;
    let key = session
        .create_task(&env, data::PROJECT_KEY, "Add a greeting file", None)
        .await?;

    let delivery = wait::eventually(&format!("{EVENT} of {key}"), RUN_TIMEOUT, || {
        delivery_of(&receiver, &key)
    })
    .await?;
    let event: Value = serde_json::from_str(&delivery.body)?;
    assert_eq!(
        event["data"]["project"]["key"],
        data::PROJECT_KEY,
        "{event}"
    );
    assert!(
        event["data"]["run"]["id"].is_string(),
        "run of the event: {event}"
    );
    let id = header(&delivery, "webhook-id")?;
    assert_eq!(event["id"], id, "{event}");
    assert!(
        id.starts_with("evt_"),
        "webhook-id `{id}` is not an event id"
    );
    verify_signature(&delivery, &public_key)?;

    // The tab shows the outcome of the last attempt.
    open_subscriptions(&session, &env).await?;
    session
        .expect_region_text(REGION, "HTTP 204", UI_TIMEOUT)
        .await
}

/// Creates a subscription to [`EVENT`] at the receiver by pressing Enter in the URL field, cancels
/// the project settings page and checks the subscription is still there; returns the public key the
/// group shows.
async fn subscribe(session: &Session, env: &E2eEnv) -> Result<String> {
    open_subscriptions(session, env).await?;
    let text = session.region_text(REGION).await?;
    let public_key = Regex::new(r"whpk_[A-Za-z0-9+/=]+")?
        .find(&text)
        .with_context(|| format!("public key in the Lifecycle events group: {text}"))?
        .as_str()
        .to_owned();
    session
        .page
        .locator(format!(
            "{REGION} button:has-text('New subscription'):visible"
        ))
        .first()
        .click(None)
        .await
        .context("open the subscription form")?;
    session
        .page
        .locator("input[name=url]")
        .fill(RECEIVER_IN_CLUSTER, None)
        .await
        .context("fill the receiver URL")?;
    session
        .page
        .locator(format!("input[name=events][value='{EVENT}']"))
        .click(None)
        .await
        .with_context(|| format!("check {EVENT}"))?;
    session
        .page
        .locator("input[name=url]")
        .press("Enter", None)
        .await
        .context("submit the subscription with Enter")?;
    session
        .expect_region_text(REGION, RECEIVER_IN_CLUSTER, UI_TIMEOUT)
        .await?;
    // Subscriptions save on their own: leaving the page without saving keeps it.
    session
        .page
        .goto(&format!("{}/projects", env.base_url), None)
        .await
        .context("leave the project settings page")?;
    open_subscriptions(session, env).await?;
    session
        .expect_region_text(REGION, RECEIVER_IN_CLUSTER, UI_TIMEOUT)
        .await?;
    Ok(public_key)
}

/// Opens "New subscription", then the edit action of the row: only one editor stays open. Saving
/// the edit with the "Save subscription" button keeps the subscription and closes the editor.
async fn edit_with_the_submit_button(session: &Session) -> Result<()> {
    let editors = session.page.locator("#subscription-editor");
    session
        .page
        .locator(format!(
            "{REGION} button:has-text('New subscription'):visible"
        ))
        .first()
        .click(None)
        .await
        .context("open the new-subscription form")?;
    editors
        .wait_for(None)
        .await
        .context("new-subscription form")?;
    session
        .page
        .locator(format!("{REGION} button[aria-label^='Edit subscription']"))
        .first()
        .click(None)
        .await
        .context("open the edit form of the row")?;
    session
        .page
        .locator("button:has-text('Save subscription')")
        .wait_for(None)
        .await
        .context("edit form")?;
    assert_eq!(editors.count().await?, 1, "one subscription editor open");
    assert_eq!(
        session.page.locator("input[name=url]").count().await?,
        1,
        "one URL field"
    );
    session
        .page
        .locator("button:has-text('Save subscription')")
        .click(None)
        .await
        .context("save the subscription with the button")?;
    wait::eventually("the editor closes after saving", UI_TIMEOUT, || async {
        Ok((editors.count().await? == 0).then_some(()))
    })
    .await?;
    session
        .expect_region_text(REGION, RECEIVER_IN_CLUSTER, UI_TIMEOUT)
        .await
}

/// Opens the Integrations tab of the project settings page through its URL.
async fn open_subscriptions(session: &Session, env: &E2eEnv) -> Result<()> {
    session
        .page
        .goto(
            &format!(
                "{}/projects/{}?tab=integrations",
                env.base_url,
                data::PROJECT_KEY
            ),
            None,
        )
        .await
        .context("open the Integrations tab of the project")?;
    session
        .expect_region_text(REGION, "Public key endpoint", UI_TIMEOUT)
        .await
}

/// The [`EVENT`] delivery of task `key`, once the receiver has it.
async fn delivery_of(receiver: &str, key: &str) -> Result<Option<Received>> {
    let received: Vec<Received> = reqwest::get(receiver)
        .await?
        .error_for_status()?
        .json()
        .await?;
    for delivery in received {
        let event: Value = serde_json::from_str(&delivery.body)
            .with_context(|| format!("event body {}", delivery.body))?;
        let is_match = event.get("type").and_then(Value::as_str) == Some(EVENT)
            && event.pointer("/data/task/key").and_then(Value::as_str) == Some(key);
        if is_match {
            return Ok(Some(delivery));
        }
    }
    Ok(None)
}

fn header<'a>(delivery: &'a Received, name: &str) -> Result<&'a str> {
    delivery
        .headers
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("header {name} in {:?}", delivery.headers))
}

/// Checks `webhook-signature` (`v1a,<base64>` over `{id}.{timestamp}.{body}`) with the
/// `whpk_<base64>` public key.
fn verify_signature(delivery: &Received, public_key: &str) -> Result<()> {
    let id = header(delivery, "webhook-id")?;
    let timestamp = header(delivery, "webhook-timestamp")?;
    let signature = header(delivery, "webhook-signature")?
        .strip_prefix("v1a,")
        .context("signature without the `v1a,` prefix")?;
    let key = STANDARD.decode(
        public_key
            .strip_prefix("whpk_")
            .context("public key without the `whpk_` prefix")?,
    )?;
    let message = format!("{id}.{timestamp}.{}", delivery.body);
    if UnparsedPublicKey::new(&ED25519, key)
        .verify(message.as_bytes(), &STANDARD.decode(signature)?)
        .is_err()
    {
        bail!("signature {signature} does not verify over {message}");
    }
    Ok(())
}
