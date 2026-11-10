//! The "Incoming webhook" group of the Integrations tab on the project settings page
//! (`forge-webhooks` spec) and `GET /projects/{key}/webhook-secret`.
//!
//! The group shows the URL a forge sends deliveries to and a masked secret. The page never holds
//! the secret: the Alpine component `secretField` (`scripts/secret-field.ts`) fetches it from the
//! endpoint when an admin shows or copies it, and every fetch is audited.

use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router, middleware};
use df_core::routing::ForgeServing;
use df_core::webhooks::WEBHOOK_SECRET_READ_ACTION;
use df_db::NewAuditEntry;
use df_db::catalog::ProjectRecord;
use maud::{Markup, html};
use secrecy::ExposeSecret;

use crate::WebState;
use crate::auth::{CurrentUser, require_admin};
use crate::components::forms::READONLY_INPUT_CLASS;
use crate::components::shell::DOCS_URL;
use crate::components::{ICON_BUTTON_CLASS, Icon, form_group, icon};
use crate::responses::{internal_error, not_found};
use crate::webhooks::WebhookSetup;

/// The masked secret: a fixed length, so it does not reveal the real one. The component gets it
/// as an argument, so the page and the script show the same mask.
const MASK: &str = "••••••••••••••••";

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/projects/{key}/webhook-secret", get(secret))
        .route_layer(middleware::from_fn(require_admin))
}

/// The group "Incoming webhook" of the Integrations tab.
pub(crate) fn group(state: &WebState, project: &ProjectRecord) -> Markup {
    form_group(
        "Incoming webhook",
        match endpoint(state, project) {
            Endpoint::Unavailable { headline, reason } => html! {
                div class="rounded-xl border border-line bg-sunk px-4 py-3 text-sm" {
                    p class="font-medium text-ink" { (headline) }
                    p class="mt-1 text-muted" { (reason) }
                }
            },
            Endpoint::Ready { plugin } => ready(state, project, &plugin),
        },
    )
}

/// Whether and how a project receives webhooks, in the order of the checks of design D5.
enum Endpoint {
    /// No webhook URL applies: `headline`, then why.
    Unavailable { headline: String, reason: String },
    /// Deliveries to `plugin` are accepted.
    Ready { plugin: String },
}

impl Endpoint {
    fn unavailable(headline: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unavailable {
            headline: headline.into(),
            reason: reason.into(),
        }
    }

    /// Routing serves the project with `plugin`, but it did not load.
    fn did_not_load(plugin: &str) -> Self {
        Self::unavailable(
            format!("The forge integration `{plugin}` did not load."),
            "Webhooks need it. Check the control plane log.",
        )
    }
}

/// The plugin that serves the project, as for the forge icon of the project list.
fn endpoint(state: &WebState, project: &ProjectRecord) -> Endpoint {
    const NO_FORGE: &str = "No forge integration serves this project.";
    if project.forge_credential_id.is_none() {
        return Endpoint::unavailable(
            NO_FORGE,
            "It has no forge credential: choose one on the General tab.",
        );
    }
    match state.forges.forge_for_repo_url(&project.repo_url) {
        ForgeServing::Unrouted => Endpoint::unavailable(
            NO_FORGE,
            "No integration is configured for the host of its repository URL.",
        ),
        ForgeServing::NotLoaded { plugin, .. } => Endpoint::did_not_load(&plugin),
        ForgeServing::Served { plugin } => match state.webhooks.setup(&plugin) {
            WebhookSetup::NotLoaded => Endpoint::did_not_load(&plugin),
            WebhookSetup::Unsupported => Endpoint::unavailable(
                "This forge integration does not support webhooks.",
                format!("The plugin `{plugin}` exports no `webhooks` interface."),
            ),
            WebhookSetup::NoSecret => Endpoint::unavailable(
                format!("Incoming webhooks are off for {plugin}."),
                format!("Set `plugins.{plugin}.webhookSecret` in the instance configuration."),
            ),
            WebhookSetup::Ready => Endpoint::Ready { plugin },
        },
    }
}

fn ready(state: &WebState, project: &ProjectRecord, plugin: &str) -> Markup {
    let path = format!("/webhooks/{plugin}");
    let url = state.public_url.as_ref().map_or_else(
        || path.clone(),
        |base| format!("{}{path}", base.as_str().trim_end_matches('/')),
    );
    let secret_url = format!("/projects/{}/webhook-secret", project.key);
    let component = format!(
        "secretField({}, {}, {})",
        serde_json::Value::from(secret_url),
        serde_json::Value::from(url.clone()),
        serde_json::Value::from(MASK)
    );
    let docs = format!("{DOCS_URL}/{}/integrations/forges/webhooks/", state.version);
    html! {
        div class="space-y-4" x-data=(component) {
            p class="text-sm text-muted" {
                "Register this URL and secret as a webhook in the " code class="font-mono text-[13px]" { (plugin) }
                " forge so that reviews, comments and merges reach Dark Factory. The secret is shared by every project this integration serves. "
                a href=(docs) class="text-accent-text underline" { "Forge webhooks" }
            }
            div class="space-y-1" {
                label for="webhook-url" class="block text-sm font-medium text-muted" { "Webhook URL" }
                div class="flex items-center gap-1" {
                    input id="webhook-url" type="text" readonly value=(url) class=(READONLY_INPUT_CLASS)
                        "x-on:keydown.enter.prevent"="";
                    (copy_button("Copy webhook URL", "url"))
                }
                @if state.public_url.is_none() {
                    p class="text-xs text-muted" {
                        "Set " code class="font-mono" { "publicUrl" } " in the instance configuration to show the full URL."
                    }
                }
            }
            div class="space-y-1" {
                label for="webhook-secret" class="block text-sm font-medium text-muted" { "Secret" }
                div class="flex items-center gap-1" {
                    input id="webhook-secret" type="text" readonly autocomplete="off" value=(MASK)
                        ":value"="shown ? secret : mask" class=(READONLY_INPUT_CLASS)
                        "x-on:keydown.enter.prevent"="";
                    button type="button" aria-label="Show secret" aria-pressed="false"
                        ":aria-label"="shown ? 'Hide secret' : 'Show secret'" ":aria-pressed"="shown"
                        "x-on:click"="toggle()" class=(ICON_BUTTON_CLASS) {
                        span x-show="!shown" { (icon(Icon::Eye)) }
                        span x-show="shown" x-cloak { (icon(Icon::EyeOff)) }
                    }
                    (copy_button("Copy secret", "secret"))
                }
            }
            p role="status" aria-live="polite" class="flex items-center gap-1.5 text-sm text-accent-text" {
                span x-show="copied" x-cloak aria-hidden="true" { (icon(Icon::Check)) }
                span x-text="copied ? 'Copied' : ''" {}
            }
            p x-show="error" x-cloak role="alert" class="text-sm text-danger" x-text="error" {}
            p x-show="!canCopy" x-cloak class="text-xs text-muted" {
                "Copying needs HTTPS. Use Show secret and copy the value by hand."
            }
        }
    }
}

/// A copy button that shows only where the browser has a secure clipboard; `what` is `url` or
/// `secret`, the value it copies. A check icon replaces the copy icon while it says "Copied".
fn copy_button(label: &str, what: &str) -> Markup {
    html! {
        button type="button" aria-label=(label) title=(label) x-show="canCopy" x-cloak
            "x-on:click"={"copy('" (what) "')"} class=(ICON_BUTTON_CLASS) {
            span x-show={"copied !== '" (what) "'"} { (icon(Icon::Copy)) }
            span x-show={"copied === '" (what) "'"} x-cloak { (icon(Icon::Check)) }
        }
    }
}

/// `GET /projects/{key}/webhook-secret`: the secret of the serving plugin as `text/plain`, never
/// cached. Every read is audited with actor, project and plugin, without the secret. Any of the
/// unavailable states of [`group`] answers `404`.
async fn secret(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
) -> Response {
    let project = match df_db::catalog::get_project_by_key(&state.pool, &key).await {
        Ok(Some(project)) => project,
        Ok(None) => return not_found("The project"),
        Err(e) => return internal_error(&e),
    };
    let Endpoint::Ready { plugin } = endpoint(&state, &project) else {
        return not_found("The webhook secret");
    };
    let Some(secret) = state.webhooks.secret(&plugin) else {
        return not_found("The webhook secret");
    };
    let entry = NewAuditEntry::new(
        user.principal(),
        user.roles.clone(),
        WEBHOOK_SECRET_READ_ACTION,
        "webhook_secret",
        plugin.clone(),
    )
    .with_details(&serde_json::json!({ "project": project.key, "plugin": plugin }));
    // The read is only allowed once the audit entry is stored.
    if let Err(e) = async { df_db::write_audit_entry(&state.pool, &entry?).await }.await {
        return internal_error(&e);
    }
    (
        [
            (CONTENT_TYPE, "text/plain; charset=utf-8"),
            (CACHE_CONTROL, "no-store"),
        ],
        secret.expose_secret().to_string(),
    )
        .into_response()
}

#[cfg(test)]
mod tests;
