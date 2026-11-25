//! The grouped persona form and the signing key of a persona: the group "Commits", the public key,
//! the copy actions and the rotation with its confirmation (`persona-commit-signing`).

use axum::http::StatusCode;
use df_core::signing_key::{SigningKey, secret_ref};
use df_core::{PersonaId, SecretError, SecretStore};
use uuid::Uuid;

use super::audit;
use crate::testkit::{TestApp, form, seed, seed_provider, text};

/// The persona form `ada` with the first seeded provider.
fn ada(provider: &str) -> String {
    form(&[
        ("display_name", "Linus"),
        ("handle", "linus"),
        ("commit_email", "linus@example.org"),
        ("system_prompt", "Be Linus."),
        ("harness", "claude-code"),
        ("model_provider_id", provider),
        ("model", "claude-sonnet-5-5"),
    ])
}

/// A persona `linus` created through the form; returns its id.
async fn create_linus(app: &TestApp, cookie: &str) -> Uuid {
    let provider = seed_provider(&app.pool).await.to_string();
    let response = app.post("/personas", &ada(&provider), cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query_scalar("SELECT id FROM personas WHERE handle = 'linus'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

async fn edit_form(app: &TestApp, cookie: &str, id: Uuid) -> String {
    let response = app.get_htmx(&format!("/personas/{id}/edit"), cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    text(response).await
}

async fn stored_key(app: &TestApp, id: Uuid) -> (String, String) {
    sqlx::query_as("SELECT signing_public_key, signing_fingerprint FROM personas WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// The positions of `needles` in `html`; panics naming a missing one.
fn positions(html: &str, needles: &[&str]) -> Vec<usize> {
    needles
        .iter()
        .map(|needle| {
            html.find(needle)
                .unwrap_or_else(|| panic!("`{needle}` missing in {html}"))
        })
        .collect()
}

fn ascending(positions: &[usize]) -> bool {
    positions.windows(2).all(|pair| pair[0] < pair[1])
}

#[tokio::test]
async fn the_persona_form_has_the_groups_identity_agent_and_commits_as_labelled_regions() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/personas/new", &cookie).await).await;

    let headings = positions(
        &html,
        &[
            "id=\"group-identity\"",
            "id=\"group-agent\"",
            "id=\"group-commits\"",
        ],
    );
    assert!(ascending(&headings), "{html}");
    for (group, name) in [
        ("identity", "Identity"),
        ("agent", "Agent"),
        ("commits", "Commits"),
    ] {
        assert!(
            html.contains(&format!("<section aria-labelledby=\"group-{group}\"")),
            "{group}: {html}"
        );
        assert!(
            html.contains(&format!(
                "id=\"group-{group}\" class=\"text-[15px] font-semibold\">{name}</h3>"
            )),
            "{group}: {html}"
        );
    }
    // Identity: avatar, display name, handle, description. Agent: prompt, harness, provider, model.
    // Commits: the commit email.
    let fields = positions(
        &html,
        &[
            "id=\"group-identity\"",
            "name=\"display_name\"",
            "name=\"handle\"",
            "name=\"description\"",
            "id=\"group-agent\"",
            "name=\"system_prompt\"",
            "name=\"harness\"",
            "name=\"model_provider_id\"",
            "name=\"model\"",
            "id=\"group-commits\"",
            "name=\"commit_email\"",
        ],
    );
    assert!(ascending(&fields), "{html}");
    // The separator is the section's top border, none above the first group.
    assert!(
        html.contains("border-t border-line pt-5 first:border-t-0"),
        "{html}"
    );
}

#[tokio::test]
async fn a_new_persona_has_no_key_and_no_rotate_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/personas/new", &cookie).await).await;

    assert!(
        html.contains("No signing key yet. Save the persona to create it."),
        "{html}"
    );
    assert!(!html.contains("Rotate key"), "{html}");
    assert!(html.contains("aria-label=\"Copy commit email\""), "{html}");
    assert!(!html.contains("Copy public key"), "{html}");
}

#[tokio::test]
async fn a_stored_persona_without_a_key_shows_the_note_and_no_rotate_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let (public,): (Option<String>,) =
        sqlx::query_as("SELECT signing_public_key FROM personas WHERE id = $1")
            .bind(seed.persona)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(public, None, "the seeded persona has no key yet");

    let html = edit_form(&app, &cookie, seed.persona).await;

    assert!(
        html.contains("No signing key yet. Save the persona to create it."),
        "{html}"
    );
    assert!(!html.contains("Rotate key"), "{html}");
    assert!(!html.contains("Copy public key"), "{html}");
}

#[tokio::test]
async fn saving_a_persona_creates_its_key_and_saving_again_keeps_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let id = create_linus(&app, &cookie).await;
    let (public, fingerprint) = stored_key(&app, id).await;
    let secret = secret_ref(PersonaId::from_uuid(id));
    let private = app.secrets.get(&secret).await.unwrap();
    assert_eq!(
        SigningKey::from_openssh(&private).unwrap().public_key(),
        public
    );
    assert!(public.starts_with("ssh-ed25519 "));
    assert!(fingerprint.starts_with("SHA256:"));

    let provider: Uuid = sqlx::query_scalar("SELECT model_provider_id FROM personas")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let saved = app
        .post(
            &format!("/personas/{id}"),
            &ada(&provider.to_string()),
            &cookie,
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);

    assert_eq!(stored_key(&app, id).await, (public, fingerprint));
    assert_eq!(app.secrets.created().len(), 1);
}

#[tokio::test]
async fn the_group_commits_shows_email_register_hint_how_to_key_fingerprint_and_rotate_in_order() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let (public, fingerprint) = stored_key(&app, id).await;

    let html = edit_form(&app, &cookie, id).await;

    let order = positions(
        &html,
        &[
            "id=\"group-commits\"",
            "name=\"commit_email\"",
            "Author and committer of this persona's commits.",
            "Register the public key and the commit email on the forge's bot account, so that the forge shows this persona's commits as verified.",
            "Verify persona commits on the forge",
            "id=\"signing-public-key\"",
            "id=\"signing-fingerprint\"",
            ">Rotate key</button>",
        ],
    );
    assert!(ascending(&order), "{html}");
    assert!(html.contains("/using/verify-persona-commits/"), "{html}");
    assert!(html.contains(&format!("value=\"{public}\"")), "{html}");
    assert!(html.contains(&format!("value=\"{fingerprint}\"")), "{html}");
    assert!(html.contains("value=\"linus@example.org\""), "{html}");
    // The key and the fingerprint are shown, not editable.
    for id in ["signing-public-key", "signing-fingerprint"] {
        let at = html.find(&format!("id=\"{id}\"")).unwrap();
        let input = &html[html[..at].rfind("<input").unwrap()..at + 300];
        assert!(input.contains("readonly"), "{id}: {input}");
    }
    assert!(
        !html.contains("Key rotated."),
        "only after a rotation: {html}"
    );
}

#[tokio::test]
async fn copy_buttons_copy_the_full_values_and_hide_without_a_secure_clipboard() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;

    let html = edit_form(&app, &cookie, id).await;

    for label in ["Copy commit email", "Copy public key", "Copy fingerprint"] {
        let at = html
            .find(&format!("aria-label=\"{label}\""))
            .unwrap_or_else(|| panic!("{label}: {html}"));
        let button = &html[html[..at].rfind("<button").unwrap()..at + 400];
        // Hidden until Alpine knows the browser has a secure clipboard.
        assert!(
            button.contains("x-show=\"canCopy\"") && button.contains("x-cloak"),
            "{label}: {button}"
        );
    }
    assert!(html.contains("x-data=\"copyValues()\""), "{html}");
    assert!(html.contains("x-text=\"copied ? 'Copied' : ''\""), "{html}");
    let script = crate::assets::script_source("copy-values.ts");
    assert!(script.contains("navigator.clipboard.writeText(value)"));
}

#[tokio::test]
async fn the_rotate_confirmation_asks_without_runs_and_names_the_runs_with_them() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;

    let idle = edit_form(&app, &cookie, id).await;

    assert!(idle.contains("Rotate the signing key of Linus?"), "{idle}");
    assert!(
        idle.contains("Dark Factory replaces the signing key of this persona. Every commit is signed with the new key from now on."),
        "{idle}"
    );
    assert!(
        idle.contains("Register the new public key on the forge's bot account. Keep the old key there so that earlier commits stay verified."),
        "{idle}"
    );
    assert!(
        idle.contains(">Cancel</button>") && idle.contains(">Rotate key</button>"),
        "{idle}"
    );
    assert!(
        idle.contains("text-danger"),
        "the confirm button is the danger style: {idle}"
    );
    assert!(!idle.contains("must be re-signed"), "{idle}");
}

#[tokio::test]
async fn the_rotate_confirmation_names_the_number_of_active_runs() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    df_db::persona_keys::sync_signing_key(&app.pool, &app.secrets, seed.persona)
        .await
        .unwrap();
    // Two active runs and an ended one, each of its own task of the persona.
    for (n, state) in ["running", "pending", "succeeded"].into_iter().enumerate() {
        let task = crate::testkit::new_task(&app.pool, &seed, &format!("Task {n}")).await;
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, $3)")
            .bind(Uuid::now_v7())
            .bind(task.id.as_uuid())
            .bind(state)
            .execute(&app.pool)
            .await
            .unwrap();
    }

    let html = edit_form(&app, &cookie, seed.persona).await;

    assert!(
        html.contains("2 runs of Linus are active. Their commits that are not pushed yet must be re-signed: the next push rejects them, and the agent is told how."),
        "{html}"
    );
    assert!(!html.contains("Keep the old key there"), "{html}");
}

#[tokio::test]
async fn rotating_shows_the_new_key_with_the_status_and_audits_both_fingerprints() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let (old_public, old_fingerprint) = stored_key(&app, id).await;

    let response = app
        .post(&format!("/personas/{id}/rotate-key"), "", &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    let (new_public, new_fingerprint) = stored_key(&app, id).await;
    assert_ne!(new_public, old_public);
    assert!(html.contains(&format!("value=\"{new_public}\"")), "{html}");
    assert!(
        html.contains(&format!("value=\"{new_fingerprint}\"")),
        "{html}"
    );
    assert!(
        html.contains("Key rotated. Register the new public key on the forge's bot account."),
        "{html}"
    );
    let secret = app
        .secrets
        .get(&secret_ref(PersonaId::from_uuid(id)))
        .await
        .unwrap();
    assert_eq!(
        SigningKey::from_openssh(&secret).unwrap().public_key(),
        new_public
    );
    let rotated = audit(&app, "persona.signing_key.rotate").await;
    assert_eq!(rotated.len(), 1);
    assert_eq!(rotated[0]["old_fingerprint"], old_fingerprint);
    assert_eq!(rotated[0]["new_fingerprint"], new_fingerprint);
}

#[tokio::test]
async fn a_failed_rotation_keeps_the_confirmation_open_with_the_error_above_the_buttons() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let before = stored_key(&app, id).await;
    app.secrets
        .fail_replace(Some(SecretError::Backend("forbidden".into())));

    let response = app
        .post(&format!("/personas/{id}/rotate-key"), "", &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("The key could not be rotated: secret store failed: forbidden. Reload the page to see the current key, then try again."),
        "{html}"
    );
    assert!(html.contains("role=\"alert\""), "{html}");
    let order = positions(
        &html,
        &[
            "Rotate the signing key of Linus?",
            "The key could not be rotated",
        ],
    );
    assert!(ascending(&order), "{html}");
    assert!(html[order[1]..].contains(">Cancel</button>"), "{html}");
    // The dialog opens by itself again; the key and the status are unchanged.
    assert!(html.contains("x-init=\"$el.showModal()\""), "{html}");
    assert!(!html.contains("Key rotated."), "{html}");
    assert_eq!(stored_key(&app, id).await, before);
    assert!(audit(&app, "persona.signing_key.rotate").await.is_empty());
}

/// The component scripts are classic scripts that share one global scope: a top-level `const` of
/// the same name in two of them is a syntax error that stops the later script.
#[test]
fn component_scripts_declare_no_top_level_name_twice() {
    let mut seen = std::collections::HashMap::new();
    for script in crate::layout::COMPONENT_SCRIPTS {
        let file = script.replace(".js", ".ts");
        for line in crate::assets::script_source(&file).lines() {
            let name = ["const ", "let ", "var "]
                .iter()
                .find_map(|keyword| line.strip_prefix(keyword))
                .and_then(|rest| rest.split([' ', ':', '=']).next());
            if let Some(name) = name
                && let Some(other) = seen.insert(name.to_owned(), file.clone())
            {
                panic!("`{name}` is declared at the top level of {other} and {file}");
            }
        }
    }
}

#[tokio::test]
async fn only_board_members_rotate_keys_and_an_unknown_persona_is_not_found() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let (outsider, _) =
        crate::auth::testing::seed_session(&app.pool, &[], 3600, Some("refresh-0")).await;

    let refused = app
        .post(&format!("/personas/{id}/rotate-key"), "", &outsider)
        .await;
    let missing = app
        .post(
            &format!("/personas/{}/rotate-key", Uuid::now_v7()),
            "",
            &cookie,
        )
        .await;

    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(audit(&app, "persona.signing_key.rotate").await.is_empty());
}

// Key verification on Forgejo.

const DOMAIN: &str = "forgejo.example.org";

async fn verify_key(app: &TestApp, cookie: &str, id: Uuid, token: &str) -> (StatusCode, String) {
    verify_key_in(app, cookie, id, token, DOMAIN).await
}

async fn verify_key_in(
    app: &TestApp,
    cookie: &str,
    id: Uuid,
    token: &str,
    domain: &str,
) -> (StatusCode, String) {
    let response = app
        .post(
            &format!("/personas/{id}/verify-key"),
            &form(&[("token", token), ("domain", domain)]),
            cookie,
        )
        .await;
    (response.status(), text(response).await)
}

/// The armored signature between the markers of a response.
fn armored(html: &str) -> String {
    let end_marker = "-----END SSH SIGNATURE-----";
    let start = html.find("-----BEGIN SSH SIGNATURE-----").expect(html);
    let end = html.find(end_marker).expect(html) + end_marker.len();
    html[start..end].to_string()
}

#[tokio::test]
async fn only_an_instance_with_the_forgejo_plugin_offers_the_verify_dialog() {
    let forgejo = TestApp::with_forges(crate::testkit::forge_routes(
        &[("forgejo", &["Forgejo.Example.org:3000"])],
        &["forgejo"],
    ))
    .await;
    let cookie = forgejo.login().await;
    let id = create_linus(&forgejo, &cookie).await;

    let html = edit_form(&forgejo, &cookie, id).await;

    let order = positions(
        &html,
        &[
            ">Verify key on Forgejo</button>",
            ">Rotate key</button>",
            "Verify the key of Linus on Forgejo",
            "In Forgejo, open the bot account's Settings → SSH / GPG keys, choose &quot;Verify&quot; next to this key and copy the token Forgejo shows.",
            "Paste the token here. Dark Factory signs it with this persona's key; paste the signature back into Forgejo.",
            "placeholder=\"Paste the token from Forgejo\"",
            "Forgejo domain",
            "value=\"forgejo.example.org\"",
            "The value after -n in the command Forgejo shows.",
            ">Sign token</button>",
        ],
    );
    assert!(ascending(&order), "{html}");

    let github = TestApp::with_forges(crate::testkit::default_forge("github")).await;
    let cookie = github.login().await;
    let id = create_linus(&github, &cookie).await;
    let html = edit_form(&github, &cookie, id).await;
    assert!(!html.contains("Verify key on Forgejo"), "{html}");
    let (status, _) = verify_key(&github, &cookie, id, "token-1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        audit(&github, "persona.signing_key.forge_verify")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_signed_token_verifies_with_forgejos_namespace_and_never_as_a_commit_signature() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let (public, fingerprint) = stored_key(&app, id).await;

    let (status, html) = verify_key(&app, &cookie, id, "  forgejo-token-123\n").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("Dark Factory signed the token with the key of Linus. Copy the signature, paste it into Forgejo and choose &quot;Verify&quot;."),
        "{html}"
    );
    assert!(html.contains("aria-label=\"Copy signature\""), "{html}");
    assert!(html.contains(">Done</button>"), "{html}");
    let signature = armored(&html);
    df_core::signing_key::verify_forgejo_token(
        &public,
        DOMAIN,
        "forgejo-token-123",
        signature.as_bytes(),
    )
    .unwrap();
    assert!(
        df_core::signing_key::verify(&public, b"forgejo-token-123", signature.as_bytes()).is_err()
    );
    let entries = audit(&app, "persona.signing_key.forge_verify").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["fingerprint"], fingerprint);
    assert_eq!(entries[0]["domain"], DOMAIN);
}

#[tokio::test]
async fn an_invalid_domain_is_not_signed_and_the_dialog_keeps_the_token_and_the_domain() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;

    for domain in ["", "git", "forgejo_http", "a b", &"a".repeat(254)] {
        let (status, html) = verify_key_in(&app, &cookie, id, "token-1", domain).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{domain:?}");
        assert!(html.contains("role=\"alert\""), "{html}");
        assert!(html.contains("The Forgejo domain"), "{html}");
        assert!(html.contains("value=\"token-1\""), "{html}");
        assert!(
            domain.is_empty() || html.contains(&format!("value=\"{domain}\"")),
            "{html}"
        );
        assert!(!html.contains("BEGIN SSH SIGNATURE"), "{html}");
    }
    assert!(
        audit(&app, "persona.signing_key.forge_verify")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn an_invalid_token_is_not_signed_and_the_dialog_keeps_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;

    for token in ["x".repeat(257), "bad\u{7}token".into(), "   ".into()] {
        let (status, html) = verify_key(&app, &cookie, id, &token).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{token:?}");
        assert!(
            html.contains("The token could not be signed: ")
                && html.contains(". Copy the token from Forgejo again."),
            "{html}"
        );
        assert!(html.contains("role=\"alert\""), "{html}");
        assert!(!html.contains("BEGIN SSH SIGNATURE"), "{html}");
    }
    assert!(
        audit(&app, "persona.signing_key.forge_verify")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_persona_without_a_stored_key_gets_nothing_signed_even_when_its_secret_exists() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    sqlx::query(
        "UPDATE personas SET signing_public_key = NULL, signing_fingerprint = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(&app.pool)
    .await
    .unwrap();

    let (status, html) = verify_key(&app, &cookie, id, "token-1").await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{html}");
    assert!(!html.contains("BEGIN SSH SIGNATURE"), "{html}");
    assert!(
        audit(&app, "persona.signing_key.forge_verify")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn only_board_members_have_a_token_signed() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let id = create_linus(&app, &cookie).await;
    let (outsider, _) =
        crate::auth::testing::seed_session(&app.pool, &[], 3600, Some("refresh-0")).await;

    let (status, _) = verify_key(&app, &outsider, id, "token-1").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        audit(&app, "persona.signing_key.forge_verify")
            .await
            .is_empty()
    );
}
