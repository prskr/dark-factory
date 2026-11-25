//! The group "Commits" of the persona form: the commit email and the signing key of the persona,
//! and the rotation of the key (`specs/commit-signing/spec.md`, "Public key on the persona page"
//! and "Key rotation").
//!
//! The Alpine component `copyValues` (`scripts/copy-values.ts`) holds the copy buttons. The part
//! below the email is the region `#signing-key`: a rotation answers with it, so the typed email of
//! the form stays as it is.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};
use df_core::signing_key::SigningKeyError;
use df_core::{ForgeKind, PersonaId};
use df_db::catalog::Actor;
use df_db::persona_keys::{KeyError, PublicSigningKey, active_run_count, rotate_signing_key};
use df_db::{DbError, NewAuditEntry, write_audit_entry};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::{INPUT_CLASS, READONLY_INPUT_CLASS, field};
use crate::components::shell::DOCS_URL;
use crate::components::{
    ButtonKind, DIALOG_CLASS, FormErrors, ICON_BUTTON_CLASS, Icon, action_class, form_group, icon,
};
use crate::responses::{internal_error, invalid, not_found};

/// What the page tells after a rotation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum Rotation {
    /// Nothing was rotated.
    #[default]
    None,
    /// The key was rotated: the status line asks for the registration of the new key.
    Done,
    /// Rotating failed with this reason: the confirmation opens again and shows it.
    Failed(String),
}

/// The signing key of a stored persona, and what the confirmation of a rotation says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeyView {
    key: PublicSigningKey,
    /// The persona's display name, as the confirmation names it.
    display_name: String,
    persona_id: Uuid,
    /// Runs of the persona that have not ended.
    active_runs: u64,
}

/// Everything the group "Commits" shows besides the email field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SigningKeySection {
    /// `None` for a new persona and for one without a key.
    key: Option<KeyView>,
    rotation: Rotation,
    /// The how-to "Verify persona commits on the forge".
    docs_url: String,
    /// The instance loads the `forgejo` plugin: the key can be verified on Forgejo.
    forgejo: bool,
    /// The prefilled "Forgejo domain": the host of the plugin's `apiUrl` or claimed host, empty
    /// without one.
    domain: String,
}

impl SigningKeySection {
    /// The section of persona `id` (none for a new one), as it is stored now; the error is the
    /// page to answer with.
    pub(super) async fn load_or_error_page(
        state: &WebState,
        id: Option<Uuid>,
        rotation: Rotation,
    ) -> Result<Self, Box<Response>> {
        Self::from_db(state, id, rotation)
            .await
            .map_err(|e| Box::new(internal_error(&e)))
    }

    async fn from_db(
        state: &WebState,
        id: Option<Uuid>,
        rotation: Rotation,
    ) -> Result<Self, DbError> {
        let docs_url = format!("{DOCS_URL}/{}/using/verify-persona-commits/", state.version);
        let forgejo = state.forges.loads(&ForgeKind::Forgejo);
        let domain = state
            .forges
            .host_of(&ForgeKind::Forgejo)
            .unwrap_or_default();
        let mut key = None;
        if let Some(id) = id
            && let Some(persona) = df_db::catalog::get_persona(&state.pool, id).await?
            && let Some(public) = persona.signing_key
        {
            key = Some(KeyView {
                key: public,
                display_name: persona.display_name,
                persona_id: id,
                active_runs: active_run_count(&state.pool, id).await?,
            });
        }
        Ok(Self {
            key,
            rotation,
            docs_url,
            forgejo,
            domain,
        })
    }
}

/// The token and the domain of `POST /personas/{id}/verify-key`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct VerifyForm {
    token: String,
    domain: String,
}

/// The group "Commits": the editable commit email with its copy button, then [`section`].
pub(super) fn commits_group(
    email: &str,
    errors: &FormErrors,
    signing: &SigningKeySection,
) -> Markup {
    form_group(
        "Commits",
        html! {
            div class="space-y-4" x-data="copyValues()" {
                (field(
                    "Commit email",
                    "commit_email",
                    errors,
                    Some("Author and committer of this persona's commits. Edits apply to runs started after saving."),
                    html! {
                        div class="flex items-center gap-1" {
                            input id="commit_email" name="commit_email" type="text" value=(email) x-ref="email"
                                class=(INPUT_CLASS)
                                aria-invalid=[errors.get("commit_email").map(|_| "true")]
                                aria-describedby=[crate::components::forms::described_by("commit_email", true, errors)];
                            (copy_button("Copy commit email", "email", "$refs.email.value"))
                        }
                    },
                ))
                (section(signing))
            }
        },
    )
}

/// `#signing-key`: the key with its copy buttons and the rotation, or the note that there is none.
pub(super) fn section(signing: &SigningKeySection) -> Markup {
    html! {
        div id="signing-key" class="space-y-4" {
            @if let Some(view) = &signing.key {
                p class="text-sm text-muted" {
                    "Register the public key and the commit email on the forge's bot account, so that the forge shows this persona's commits as verified."
                }
                p class="text-sm" {
                    a href=(signing.docs_url) class="text-accent-text underline" { "Verify persona commits on the forge" }
                }
                (key_field("Public key", "signing-public-key", &view.key.public_key, "Copy public key", "public-key"))
                (key_field("Fingerprint", "signing-fingerprint", &view.key.fingerprint, "Copy fingerprint", "fingerprint"))
                p role="status" aria-live="polite" class="flex items-center gap-1.5 text-sm text-accent-text" {
                    span x-show="copied" x-cloak aria-hidden="true" { (icon(Icon::Check)) }
                    span x-text="copied ? 'Copied' : ''" {}
                }
                p x-show="error" x-cloak role="alert" class="text-sm text-danger" x-text="error" {}
                @if signing.rotation == Rotation::Done {
                    p role="status" class="flex items-center gap-1.5 text-sm font-medium text-accent-text" {
                        (icon(Icon::Check))
                        "Key rotated. Register the new public key on the forge's bot account."
                    }
                }
                div class="flex flex-col gap-2 md:flex-row" {
                    @if signing.forgejo {
                        button type="button" "x-on:click"="$refs.verifyDialog.showModal()"
                            class={(action_class(ButtonKind::Secondary)) " max-md:min-h-12"} { "Verify key on Forgejo" }
                    }
                    button type="button" "x-on:click"="$refs.rotateDialog.showModal()"
                        class={(action_class(ButtonKind::Secondary)) " max-md:min-h-12"} { "Rotate key" }
                }
                @if signing.forgejo {
                    (verify_dialog(view, &ForgeToken::Empty { domain: signing.domain.clone() }))
                }
                (rotate_dialog(view, &signing.rotation))
            } @else {
                p class="text-sm text-muted" { "No signing key yet. Save the persona to create it." }
            }
        }
    }
}

/// `POST /personas/{id}/verify-key`: signs the Forgejo token with the persona's key. Only an
/// instance with the `forgejo` plugin serves it (`404` otherwise). The answer is the dialog body
/// with the signature; an invalid token or domain is a `422` with the body that keeps both.
pub(super) async fn verify_forgejo(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    Form(form): Form<VerifyForm>,
) -> Response {
    if !state.forges.loads(&ForgeKind::Forgejo) {
        return not_found("Verifying a key on Forgejo");
    }
    // Without a stored key nothing is signed and nothing audited, whatever the Secret holds.
    let signing =
        match SigningKeySection::load_or_error_page(&state, Some(id), Rotation::None).await {
            Ok(signing) => signing,
            Err(response) => return *response,
        };
    let Some(view) = &signing.key else {
        return not_found("The signing key");
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let VerifyForm { token, domain } = form;
    let result = sign_token(&state, actor, id, &domain, &token).await;
    let failed = |reason| ForgeToken::Failed {
        token: token.clone(),
        domain: domain.clone(),
        reason,
    };
    let outcome = match result {
        Ok(signature) => ForgeToken::Signed(signature),
        Err(KeyError::Db(DbError::NotFound(_))) => return not_found("The persona"),
        Err(KeyError::Db(e)) => return internal_error(&e),
        Err(KeyError::Key(SigningKeyError::InvalidToken(reason))) => failed(Reason::Token(reason)),
        Err(KeyError::Key(SigningKeyError::InvalidDomain(reason))) => {
            failed(Reason::Domain(reason))
        }
        Err(e) => {
            tracing::error!(error = %e, persona = %id, "signing a Forgejo token failed");
            failed(Reason::Other(e.to_string()))
        }
    };
    let failed = matches!(outcome, ForgeToken::Failed { .. });
    let body = verify_body(view, &outcome);
    if failed {
        invalid(body)
    } else {
        body.into_response()
    }
}

/// `POST /personas/{id}/rotate-key`: a board member confirmed the rotation. The answer is
/// `#signing-key` with the new key and the status; a failure is a `422` with the section whose
/// confirmation shows the error.
pub(super) async fn rotate(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
) -> Response {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let rotation = match rotate_signing_key(&state.pool, state.secrets.as_ref(), actor, id).await {
        Ok(_) => Rotation::Done,
        Err(KeyError::Db(DbError::NotFound(_))) => return not_found("The persona"),
        Err(KeyError::Db(e)) => return internal_error(&e),
        Err(e) => {
            tracing::error!(error = %e, persona = %id, "rotating the signing key failed");
            Rotation::Failed(e.to_string())
        }
    };
    let failed = matches!(rotation, Rotation::Failed(_));
    let signing = match SigningKeySection::load_or_error_page(&state, Some(id), rotation).await {
        Ok(signing) => signing,
        Err(response) => return *response,
    };
    let body = section(&signing);
    if failed {
        invalid(body)
    } else {
        body.into_response()
    }
}

/// What the verify dialog shows.
enum ForgeToken {
    /// The token field empty, the domain field prefilled.
    Empty { domain: String },
    /// The armored signature of the token.
    Signed(String),
    /// Signing failed: the entered token and domain stay, and the reason shows above the buttons.
    Failed {
        token: String,
        domain: String,
        reason: Reason,
    },
}

/// Which input a refusal is about.
enum Reason {
    Token(String),
    Domain(String),
    Other(String),
}

/// Signs `token` with the key of persona `id`, read from its Secret, with `domain` as namespace
/// (design D7), and audits it as `persona.signing_key.forge_verify` with the domain. Nothing is
/// signed, and nothing audited, for an invalid token or domain, or a persona without a key.
async fn sign_token(
    state: &WebState,
    actor: Actor<'_>,
    id: Uuid,
    domain: &str,
    token: &str,
) -> Result<String, KeyError> {
    let persona = df_db::catalog::get_persona(&state.pool, id)
        .await?
        .ok_or_else(|| KeyError::Db(DbError::NotFound(format!("persona `{id}`"))))?;
    let key = df_core::signing_key::load(state.secrets.as_ref(), PersonaId::from_uuid(id)).await?;
    let signature = key.sign_forgejo_token(domain, token)?;
    let entry = NewAuditEntry::new(
        actor.principal.clone(),
        actor.roles.to_vec(),
        "persona.signing_key.forge_verify",
        "persona",
        persona.handle,
    )
    .with_details(&serde_json::json!({
        "id": id,
        "fingerprint": key.fingerprint(),
        "domain": domain.trim(),
    }))?;
    write_audit_entry(&state.pool, &entry).await?;
    Ok(signature)
}

/// A read-only value with its copy button; it shows on one line and copies in full.
fn key_field(label: &str, id: &str, value: &str, copy_label: &str, what: &str) -> Markup {
    let class = format!("{READONLY_INPUT_CLASS} truncate");
    html! {
        div class="space-y-1" {
            label for=(id) class="block text-sm font-medium text-muted" { (label) }
            div class="flex items-center gap-1" {
                input id=(id) type="text" readonly value=(value) class=(class) title=(value)
                    "x-on:keydown.enter.prevent"="";
                (copy_button(copy_label, what, &serde_json::Value::from(value).to_string()))
            }
        }
    }
}

/// A copy button that shows only where the browser has a secure clipboard (as the one of the
/// webhook URL); `value` is the Alpine expression of what it copies. A check icon replaces the
/// copy icon while the status says "Copied".
fn copy_button(label: &str, what: &str, value: &str) -> Markup {
    html! {
        button type="button" aria-label=(label) title=(label) x-show="canCopy" x-cloak
            "x-on:click"={"copy('" (what) "', " (value) ")"} class=(ICON_BUTTON_CLASS) {
            span x-show={"copied !== '" (what) "'"} { (icon(Icon::Copy)) }
            span x-show={"copied === '" (what) "'"} x-cloak { (icon(Icon::Check)) }
        }
    }
}

/// The confirmation of "Rotate key", built like the one of "Reset workspace…": it names the
/// persona and what a rotation does. With active runs the second paragraph tells that their
/// unpushed commits must be re-signed. After a failed rotation it opens by itself and shows the
/// error between the paragraphs and the buttons.
fn rotate_dialog(view: &KeyView, rotation: &Rotation) -> Markup {
    let action = format!("/personas/{}/rotate-key", view.persona_id);
    let name = &view.display_name;
    html! {
        dialog x-ref="rotateDialog" aria-labelledby="rotate-title"
            x-init=[matches!(rotation, Rotation::Failed(_)).then_some("$el.showModal()")]
            "x-on:click"="$event.target === $el && $el.close()" class=(DIALOG_CLASS) {
            div class="rounded-2xl border border-line bg-panel p-5 shadow-lg" {
                h2 id="rotate-title" class="text-lg font-semibold" { "Rotate the signing key of " (name) "?" }
                p class="mt-3 text-sm font-normal max-md:text-[15px]" {
                    "Dark Factory replaces the signing key of this persona. Every commit is signed with the new key from now on."
                }
                p class="mt-3 text-sm font-normal max-md:text-[15px]" {
                    @match view.active_runs {
                        0 => "Register the new public key on the forge's bot account. Keep the old key there so that earlier commits stay verified.",
                        1 => { "1 run of " (name) " is active. Its commits that are not pushed yet must be re-signed: the next push rejects them, and the agent is told how." }
                        runs => { (runs) " runs of " (name) " are active. Their commits that are not pushed yet must be re-signed: the next push rejects them, and the agent is told how." }
                    }
                }
                @if let Rotation::Failed(reason) = rotation {
                    p role="alert" class="mt-3 text-sm text-danger" {
                        "The key could not be rotated: " (reason.trim_end_matches('.')) ". Reload the page to see the current key, then try again."
                    }
                }
                div class="mt-5 flex flex-col-reverse gap-2 md:flex-row md:justify-end" {
                    button type="button" "x-on:click"="$refs.rotateDialog.close()" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                    button type="button" hx-post=(action) hx-target="#signing-key" hx-swap="outerHTML"
                        class=(action_class(ButtonKind::Danger)) { "Rotate key" }
                }
            }
        }
    }
}

/// The dialog "Verify the key of <persona> on Forgejo" (design D7): a token field, and after
/// "Sign token" the armored signature in the same dialog. `#forge-verify-body` is what a request
/// replaces.
fn verify_dialog(view: &KeyView, state: &ForgeToken) -> Markup {
    let name = &view.display_name;
    html! {
        dialog x-ref="verifyDialog" aria-labelledby="verify-title"
            "x-on:click"="$event.target === $el && $el.close()" class=(DIALOG_CLASS) {
            div class="rounded-2xl border border-line bg-panel p-5 shadow-lg" {
                h2 id="verify-title" class="text-lg font-semibold" { "Verify the key of " (name) " on Forgejo" }
                (verify_body(view, state))
            }
        }
    }
}

fn verify_body(view: &KeyView, state: &ForgeToken) -> Markup {
    let action = format!("/personas/{}/verify-key", view.persona_id);
    let name = &view.display_name;
    let paragraph = "mt-3 text-sm font-normal max-md:text-[15px]";
    let buttons = "mt-5 flex flex-col-reverse gap-2 md:flex-row md:justify-end";
    html! {
        div id="forge-verify-body" {
            @if let ForgeToken::Signed(signature) = state {
                p class=(paragraph) {
                    "Dark Factory signed the token with the key of " (name) ". Copy the signature, paste it into Forgejo and choose \"Verify\"."
                }
                div class="mt-3 space-y-1" {
                    label for="forge-signature" class="block text-sm font-medium text-muted" { "Signature" }
                    div class="flex items-start gap-1" {
                        textarea id="forge-signature" readonly rows=(signature.lines().count())
                            class={(READONLY_INPUT_CLASS) " whitespace-pre overflow-x-auto"} { (signature) }
                        (copy_button("Copy signature", "signature", &serde_json::Value::from(signature.as_str()).to_string()))
                    }
                }
                div class=(buttons) {
                    button type="button" "x-on:click"="$refs.verifyDialog.close()" class=(action_class(ButtonKind::Primary)) { "Done" }
                }
            } @else {
                @let (token, domain, reason) = match state {
                    ForgeToken::Failed { token, domain, reason } => (token.as_str(), domain.as_str(), Some(reason)),
                    ForgeToken::Empty { domain } => ("", domain.as_str(), None),
                    // The signed state has its own branch above.
                    ForgeToken::Signed(_) => ("", "", None),
                };
                p class=(paragraph) {
                    "In Forgejo, open the bot account's Settings → SSH / GPG keys, choose \"Verify\" next to this key and copy the token Forgejo shows."
                }
                p class=(paragraph) {
                    "Paste the token here. Dark Factory signs it with this persona's key; paste the signature back into Forgejo."
                }
                div class="mt-3 space-y-1" {
                    label for="forge-token" class="block text-sm font-medium text-muted" { "Token" }
                    // No form owns the field: the persona form does not save it, and Enter must not submit that form.
                    input id="forge-token" name="token" form="forge-token-owner" type="text" value=(token)
                        placeholder="Paste the token from Forgejo" autocomplete="off"
                        "x-on:keydown.enter.prevent"="" class=(INPUT_CLASS);
                }
                div class="mt-3 space-y-1" {
                    label for="forge-domain" class="block text-sm font-medium text-muted" { "Forgejo domain" }
                    input id="forge-domain" name="domain" form="forge-token-owner" type="text" value=(domain)
                        autocomplete="off" aria-describedby="forge-domain-hint"
                        "x-on:keydown.enter.prevent"="" class=(INPUT_CLASS);
                    p id="forge-domain-hint" class="text-xs text-faint" { "The value after -n in the command Forgejo shows." }
                }
                @if let Some(reason) = reason {
                    p role="alert" class="mt-3 text-sm text-danger" {
                        @match reason {
                            Reason::Token(reason) => { "The token could not be signed: " (reason.trim_end_matches('.')) ". Copy the token from Forgejo again." }
                            Reason::Domain(reason) => { (reason.trim_end_matches('.')) "." }
                            Reason::Other(reason) => { "The token could not be signed: " (reason.trim_end_matches('.')) "." }
                        }
                    }
                }
                div class=(buttons) {
                    button type="button" "x-on:click"="$refs.verifyDialog.close()" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                    button type="button" hx-post=(action) hx-include="#forge-token, #forge-domain" hx-target="#forge-verify-body" hx-swap="outerHTML"
                        class=(action_class(ButtonKind::Primary)) { "Sign token" }
                }
            }
        }
    }
}
