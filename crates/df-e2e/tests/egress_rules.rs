//! Editing egress rules in a real browser (`just e2e-local`): the rules page, the Egress tab of the
//! project settings page and "Make global". Every test uses hosts of its own under
//! `example.org` and removes the rules it created. Needs the environment of `E2eEnv`, so the tests
//! are ignored by default.

use std::sync::Arc;

use anyhow::{Context, Result};
use df_e2e::browser::{Session, assert_no_sideways_scroll, button, button_in};
use df_e2e::fixture::{self, World};
use df_e2e::{E2eEnv, data};
use playwright_rs::protocol::SelectOption;
use playwright_rs::{Viewport, expect, expect_page};

/// Narrower than the 1440 px of the design, wider than a phone: the header of the rules page used
/// to wrap here.
const NARROW_DESKTOP: Viewport = Viewport {
    width: 1100,
    height: 900,
};
const DESKTOP: Viewport = Viewport {
    width: 1440,
    height: 900,
};
const PHONE: Viewport = Viewport {
    width: 390,
    height: 844,
};

/// The two projects of the seed that only these tests use.
const FIRST: &str = data::EGRESS_PROJECT_KEY;
const SECOND: &str = data::EGRESS_OTHER_PROJECT_KEY;

/// The table row of the rule `target` (`host:port`) on the rules page or the Egress tab.
fn row(session: &Session, target: &str) -> playwright_rs::protocol::Locator {
    session.page.locator(format!(
        "tr:has(button[aria-label='Edit egress rule {target}'])"
    ))
}

/// The underlined action in the hint line of the rule `target` ("Make global", "Remove"). The
/// confirmation has a button of the same name, so the role does not tell them apart.
fn hint_action(session: &Session, target: &str) -> playwright_rs::protocol::Locator {
    session.page.locator(format!(
        "tr:has(button[aria-label='Edit egress rule {target}']) button.underline"
    ))
}

async fn open_rules_page(env: &E2eEnv, session: &Session) -> Result<()> {
    session
        .page
        .goto(&format!("{}/egress/allowlist", env.base_url), None)
        .await?;
    Ok(())
}

async fn open_egress_tab(env: &E2eEnv, session: &Session, key: &str) -> Result<()> {
    session
        .page
        .goto(&format!("{}/projects/{key}?tab=egress", env.base_url), None)
        .await?;
    // The rules load when the tab is first shown.
    expect(session.page.locator("#project-egress"))
        .to_be_visible()
        .await
        .with_context(|| format!("the Egress tab of {key} loads its rules"))
}

/// Opens the "New egress rule" dialog on the Egress tab of project `key` (it has "Applies to" set
/// to the project).
async fn open_new_rule_dialog(session: &Session, key: &str) -> Result<()> {
    session
        .page
        .locator(format!(
            "#project-egress button[hx-get$='/host/new?return=project:{key}']"
        ))
        .click(None)
        .await
        .with_context(|| format!("open New egress rule on the Egress tab of {key}"))
}

/// Adds a rule for `host` on the Egress tab of project `key` through the dialog.
async fn add_rule_on_tab(session: &Session, key: &str, host: &str) -> Result<()> {
    open_new_rule_dialog(session, key).await?;
    session.page.locator("#host").fill(host, None).await?;
    button(&session.page, "Add rule").click(None).await?;
    Ok(())
}

/// Opens the "New egress rule" dialog by its button (`hx-get`), fills the host and saves.
async fn add_rule(session: &Session, new_url: &str, host: &str) -> Result<()> {
    session
        .page
        .locator(format!("button[hx-get='{new_url}']:visible"))
        // `.first()`: the page has the button in the header and in an empty state; either opens it.
        .first()
        .click(None)
        .await
        .context("open the New egress rule dialog")?;
    session.page.locator("#host").fill(host, None).await?;
    button(&session.page, "Add rule").click(None).await?;
    Ok(())
}

/// egress-allowlist, "Rules table" (scenario "Edit button") and "Edit dialog" (scenarios "Fix a
/// typo", "Widen to instance"): a rule with a typo is edited in place, then widened to every
/// project, and both changes show in the table.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_host_typo_is_fixed_and_a_project_rule_is_widened_on_the_rules_page() -> Result<()> {
    fixture::run(
        "a_host_typo_is_fixed_and_a_project_rule_is_widened_on_the_rules_page",
        typo_and_widen_body,
    )
    .await
}

async fn typo_and_widen_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    let host = format!("{}.example.org", world.unique("fixed"));
    let typo = host.replace("fixed", "fixxed");
    session.page.set_viewport_size(DESKTOP).await?;
    open_rules_page(env, session).await?;
    add_rule(session, "/egress/allowlist/host/new", &typo).await?;
    expect(row(session, &format!("{typo}:443")))
        .to_contain_text("Everywhere")
        .await
        .context("the new rule is listed")?;

    // Fix the typo: the rule keeps its row, the old host is gone.
    session
        .page
        .locator(format!("button[aria-label='Edit egress rule {typo}:443']"))
        .click(None)
        .await
        .context("open the edit dialog")?;
    expect(session.page.locator("#host"))
        .to_have_value(&typo)
        .await
        .context("the dialog is prefilled")?;
    session.page.locator("#host").fill(&host, None).await?;
    button(&session.page, "Save rule").click(None).await?;
    expect(row(session, &format!("{host}:443")))
        .to_be_visible()
        .await
        .context("the fixed host is listed")?;
    expect(row(session, &format!("{typo}:443")))
        .to_have_count(0)
        .await
        .context("the typo is gone")?;

    // Narrow it to a project, then widen it to the instance again.
    for (label, shown) in [
        (
            format!("{} ({FIRST})", data::EGRESS_PROJECT_NAME),
            format!("Project {FIRST}"),
        ),
        (
            "Every project (instance)".to_string(),
            "Everywhere".to_string(),
        ),
    ] {
        session
            .page
            .locator(format!("button[aria-label='Edit egress rule {host}:443']"))
            .click(None)
            .await?;
        session
            .page
            .locator("#project")
            .select_option(SelectOption::Label(label), None)
            .await?;
        button(&session.page, "Save rule").click(None).await?;
        expect(row(session, &format!("{host}:443")))
            .to_contain_text(&shown)
            .await
            .with_context(|| format!("the scope changed to {shown}"))?;
    }

    session
        .page
        .locator(format!(
            "button[aria-label='Delete egress rule {host}:443']"
        ))
        .click(None)
        .await?;
    expect(row(session, &format!("{host}:443")))
        .to_have_count(0)
        .await
        .context("the rule is removed again")
}

/// egress-allowlist, "Rules table": below 1440 px the header actions stay at the right of the row,
/// like the header buttons of the other pages, instead of wrapping onto a line of their own.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn the_header_actions_of_the_rules_page_stay_at_the_right_below_1440_px() -> Result<()> {
    fixture::run(
        "the_header_actions_of_the_rules_page_stay_at_the_right_below_1440_px",
        header_body,
    )
    .await
}

async fn header_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    session.page.set_viewport_size(NARROW_DESKTOP).await?;
    open_rules_page(env, session).await?;
    let header = session.page.locator("#config-list > header");
    expect(header.clone())
        .to_be_visible()
        .await
        .context("the header of the rules page is shown")?;
    let header = header.bounding_box().await?.context("box of the header")?;
    let live = session
        .page
        .locator("#config-list > header a[href='/egress/log']")
        .bounding_box()
        .await?
        .context("box of Live traffic")?;
    let new = session
        .page
        .locator("#config-list > header button[hx-get='/egress/allowlist/host/new']")
        .bounding_box()
        .await?
        .context("box of New egress rule")?;
    let right = header.x + header.width;
    assert!(
        (right - (new.x + new.width)).abs() < 2.0,
        "New egress rule ends at {}, the header at {right}",
        new.x + new.width
    );
    assert!(
        live.y + live.height <= header.y + header.height + 1.0,
        "the actions stay in the row of the title: {live:?} in {header:?}"
    );
    Ok(())
}

/// egress-allowlist, "Project egress rules" and "Instance rules on the project egress tab": a
/// rule is added, edited and deleted on the Egress tab, the instance section opens, and on a
/// phone the rules are cards that do not scroll the page sideways.
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn a_rule_is_added_edited_and_deleted_on_the_egress_tab_and_the_phone_has_no_sideways_scroll()
-> Result<()> {
    fixture::run(
        "a_rule_is_added_edited_and_deleted_on_the_egress_tab_and_the_phone_has_no_sideways_scroll",
        egress_tab_body,
    )
    .await
}

async fn egress_tab_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    let host = format!("{}.example.org", world.unique("tab"));
    let target = format!("{host}:443");
    session.page.set_viewport_size(DESKTOP).await?;
    open_egress_tab(env, session, FIRST).await?;
    expect(button(&session.page, "Save project"))
        .to_be_hidden()
        .await
        .context("the Egress tab has no Save project")?;

    // New egress rule: the project is preselected.
    open_new_rule_dialog(session, FIRST).await?;
    expect(session.page.locator("#project"))
        .to_have_value(FIRST)
        .await
        .context("Applies to is the project")?;
    session.page.locator("#host").fill(&host, None).await?;
    button(&session.page, "Add rule").click(None).await?;
    expect(row(session, &target))
        .to_contain_text(&format!("Project {FIRST}"))
        .await
        .context("the rule is listed on the tab")?;
    assert_eq!(
        session.page.url(),
        format!("{}/projects/{FIRST}?tab=egress", env.base_url),
        "the page stays on the tab"
    );

    // Edit: the port changes in place.
    session
        .page
        // `tr`: the cards of a phone have the button too.
        .locator(format!("tr button[aria-label='Edit egress rule {target}']"))
        .click(None)
        .await?;
    session.page.locator("#port").fill("8443", None).await?;
    button(&session.page, "Save rule").click(None).await?;
    expect(row(session, &format!("{host}:8443")))
        .to_be_visible()
        .await
        .context("the edited rule is listed")?;

    duplicate_is_refused_in_the_dialog(&world, &host).await?;
    instance_section_and_phone(&world, &host).await
}

/// egress-allowlist, "Edit conflicts": an edit of a second rule on the Egress tab that duplicates
/// the rule `host:8443` is refused in the dialog's own form; the project form stays as it is.
async fn duplicate_is_refused_in_the_dialog(world: &World, host: &str) -> Result<()> {
    let session = &world.session;
    let other = format!("{}.example.org", world.unique("tab-other"));
    add_rule_on_tab(session, FIRST, &other).await?;
    expect(row(session, &format!("{other}:443")))
        .to_be_visible()
        .await
        .context("the second rule is listed")?;
    session
        .page
        .locator(format!(
            "tr button[aria-label='Edit egress rule {other}:443']"
        ))
        .click(None)
        .await?;
    session.page.locator("#host").fill(host, None).await?;
    session.page.locator("#port").fill("8443", None).await?;
    button(&session.page, "Save rule").click(None).await?;
    expect(session.page.locator("#rule-form"))
        .to_contain_text("this entry already exists")
        .await
        .context("the dialog shows the duplicate message")?;
    expect(session.page.locator("form#form"))
        .to_have_count(1)
        .await
        .context("the project form is still on the page")?;
    expect(session.page.locator("#project-egress"))
        .to_be_visible()
        .await
        .context("the Egress tab is untouched")?;
    button_in(&session.page.locator("#rule-form"), "Cancel")
        .click(None)
        .await
        .context("close the dialog")?;
    session
        .page
        .locator(format!(
            "tr button[aria-label='Delete egress rule {other}:443']"
        ))
        .click(None)
        .await?;
    expect(row(session, &format!("{other}:443")))
        .to_have_count(0)
        .await
        .context("the second rule is deleted")
}

/// The collapsed instance section opens, and a phone shows the rules as cards that do not scroll
/// the page sideways; the rule `host:8443` is deleted from its card.
async fn instance_section_and_phone(world: &World, host: &str) -> Result<()> {
    let (env, session) = (&world.env, &world.session);

    // The instance rules are collapsed until the section is opened.
    let instance = session.page.locator("#project-egress-instance");
    expect(instance.locator("table"))
        .to_be_hidden()
        .await
        .context("the instance section is collapsed")?;
    instance
        .locator("button[aria-expanded]")
        .click(None)
        .await?;
    expect(instance.locator("button[aria-expanded]"))
        .to_have_attribute("aria-expanded", "true")
        .await
        .context("the instance section is expanded")?;

    // A phone shows the rules as cards.
    session.page.set_viewport_size(PHONE).await?;
    open_egress_tab(env, session, FIRST).await?;
    let card = session.page.locator(format!(
        "#project-egress li:has(button[aria-label='Edit egress rule {host}:8443'])"
    ));
    expect(card.clone())
        .to_contain_text(&format!("Allow · 8443 · Project {FIRST}"))
        .await
        .context("the card names effect, port and scope")?;
    assert_no_sideways_scroll(&session.page, "on the phone card list").await?;

    // Delete from the card.
    card.locator(format!(
        "button[aria-label='Delete egress rule {host}:8443']"
    ))
    .click(None)
    .await?;
    expect(card)
        .to_have_count(0)
        .await
        .context("the rule is deleted")?;
    // No check of the empty state: the project is shared with the Make global test, which runs at
    // the same time and may have a rule of its own on it (a df-web test covers the empty state).
    expect_page(&session.page)
        .to_have_url_regex("tab=egress")
        .await
        .context("the phone stays on the Egress tab after the delete")?;
    assert_no_sideways_scroll(&session.page, "on the phone tab after the delete").await
}

/// egress-allowlist, "Same rule in other projects": two projects with the same rule, "Make
/// global" on one tab asks in the row, "Cancel" changes nothing, the confirmation replaces both
/// rules by one instance rule that both tabs list in "Also applies (instance)".
#[tokio::test]
#[ignore = "needs the cluster of `just e2e-local`"]
async fn make_global_replaces_the_rules_of_two_projects_by_one_instance_rule() -> Result<()> {
    fixture::run(
        "make_global_replaces_the_rules_of_two_projects_by_one_instance_rule",
        make_global_body,
    )
    .await
}

async fn make_global_body(world: Arc<World>) -> Result<()> {
    let (env, session) = (&world.env, &world.session);
    let host = format!("{}.example.org", world.unique("global"));
    let target = format!("{host}:443");
    session.page.set_viewport_size(DESKTOP).await?;
    for key in [FIRST, SECOND] {
        open_egress_tab(env, session, key).await?;
        add_rule_on_tab(session, key, &host).await?;
        expect(row(session, &target))
            .to_be_visible()
            .await
            .with_context(|| format!("the rule is listed on the tab of {key}"))?;
    }

    open_egress_tab(env, session, FIRST).await?;
    expect(row(session, &target))
        .to_contain_text(&format!("Also allowed for project {SECOND}"))
        .await
        .context("the hint names the other project")?;
    hint_action(session, &target)
        .click(None)
        .await
        .context("ask")?;
    let confirmation = format!(
        "Make global? The rules of {FIRST} and {SECOND} are replaced by one instance rule; every project may then reach it."
    );
    // `.first()`: the text is in the table row and in the card of a phone; the row comes first.
    expect(session.page.get_by_text(&confirmation, true).first())
        .to_be_visible()
        .await
        .context("the row asks first")?;
    button(&session.page, "Cancel").click(None).await?;
    expect(row(session, &target))
        .to_contain_text("Also allowed for project")
        .await
        .context("Cancel keeps the rule")?;

    expect(hint_action(session, &target))
        .to_be_visible()
        .await
        .context("the hint is back after Cancel")?;
    hint_action(session, &target)
        .click(None)
        .await
        .context("ask again")?;
    // By `hx-post`: the hint and the confirmation have the same accessible name.
    session
        .page
        .locator("button[hx-post='/egress/allowlist/make-global']:visible")
        .click(None)
        .await
        .context("confirm")?;
    expect(row(session, &target))
        .to_have_count(0)
        .await
        .context("the project rule is gone")?;
    for key in [FIRST, SECOND] {
        open_egress_tab(env, session, key).await?;
        expect(row(session, &target))
            .to_have_count(0)
            .await
            .with_context(|| format!("{key} has no rule of its own"))?;
        let instance = session.page.locator("#project-egress-instance");
        instance
            .locator("button[aria-expanded]")
            .click(None)
            .await?;
        expect(instance.locator(format!("a[href='/egress/allowlist?highlight={target}']")))
            .to_be_visible()
            .await
            .with_context(|| format!("{key} lists the instance rule"))?;
    }

    // Remove the instance rule again.
    session
        .page
        .goto(
            &format!("{}/egress/allowlist?highlight={target}", env.base_url),
            None,
        )
        .await?;
    session
        .page
        .locator(format!("button[aria-label='Delete egress rule {target}']"))
        .click(None)
        .await?;
    expect(row(session, &target))
        .to_have_count(0)
        .await
        .context("the instance rule is removed")
}
