//! The "Lifecycle events" tab of the project edit form (`event-subscriptions` spec): lists and
//! changes the subscriptions of a project, for users with the `admin` role only.
//!
//! The tab loads its content when it is first shown ([`tab_panel`]). The editor sits in the tab
//! and posts to its own `<form id="subscription-form">` ([`form_anchor`]) because the project form
//! around the tab cannot be nested. Every change takes effect at once, independent of saving the
//! project.

use axum::extract::{Path, RawForm, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Router, middleware};
use df_core::EventType;
use df_core::public_address::{ProjectTargets, literal_host_is_public};
use df_db::DbError;
use df_db::catalog::{Actor, ProjectRecord};
use df_db::event_subscriptions::{self, Subscription, SubscriptionInput};
use maud::{Markup, html};
use url::Url;
use uuid::Uuid;

use crate::WebState;
use crate::auth::{CurrentUser, is_htmx, require_admin};
use crate::components::{
    ButtonKind, FormErrors, ICON_BUTTON_CLASS, Icon, action_class, empty_panel, field_error,
    form_error, icon, mono_text_field_of_form,
};
use crate::responses::{internal_error, invalid, not_found};

/// The endpoint receivers fetch the instance public key from.
const PUBLIC_KEY_PATH: &str = "/lifecycle-events/public-key";
const INVALID_URL: &str = "Enter a valid URL, for example https://hooks.example.org/events.";
/// The empty form the editor's inputs belong to (`form` attribute).
const FORM_ID: &str = "subscription-form";

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/projects/{key}/subscriptions", get(tab).post(create))
        .route("/projects/{key}/subscriptions/new", get(new_form))
        .route(
            "/projects/{key}/subscriptions/{id}",
            post(update).delete(remove),
        )
        .route("/projects/{key}/subscriptions/{id}/edit", get(edit_form))
        .route_layer(middleware::from_fn(require_admin))
}

/// Panel of the tab in the project form: a placeholder that fetches the tab on first display.
pub(crate) fn tab_panel(key: &str) -> Markup {
    html! {
        div hx-get=(base_path(key)) hx-trigger="intersect once" hx-swap="outerHTML" class="text-sm text-muted" {
            "Loading lifecycle events…"
        }
    }
}

/// The empty form the editor's inputs are associated with. It sits next to the project form,
/// because forms cannot be nested.
pub(crate) fn form_anchor() -> Markup {
    html! { form id=(FORM_ID) {} }
}

/// The submitted form: the URL and the checked event types, as typed.
#[derive(Debug, Default)]
struct SubscriptionForm {
    url: String,
    events: Vec<String>,
}

impl SubscriptionForm {
    /// Reads `url` and every `events` value; the checkboxes repeat the field name.
    fn parse(body: &[u8]) -> Self {
        let mut form = Self::default();
        for (name, value) in url::form_urlencoded::parse(body) {
            match name.as_ref() {
                "url" => form.url = value.into_owned(),
                "events" => form.events.push(value.into_owned()),
                _ => {}
            }
        }
        form
    }

    fn of(subscription: &Subscription) -> Self {
        Self {
            url: subscription.url.clone(),
            events: subscription
                .events
                .iter()
                .map(|event| event.as_str().to_owned())
                .collect(),
        }
    }
}

/// Checks the form: an `https` URL whose host is not a literal internal address, and known event
/// types. The delivery checks the resolved addresses again before each attempt. With
/// [`ProjectTargets::Unrestricted`] any valid URL is accepted. A URL with a username or password
/// is always refused: the URL is written to the append-only audit log.
fn validate(
    form: &SubscriptionForm,
    targets: ProjectTargets,
) -> Result<SubscriptionInput, FormErrors> {
    let mut errors = FormErrors::default();
    let url = form.url.trim();
    let restricted = targets == ProjectTargets::PublicHttps;
    match Url::parse(url) {
        Ok(parsed) if parsed.host().is_none() => errors.add("url", INVALID_URL),
        Ok(parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            errors.add("url", "The URL must not contain a username or password.");
        }
        Ok(parsed) if restricted && parsed.scheme() != "https" => {
            errors.add("url", "Project subscriptions need https.");
        }
        Ok(parsed) if restricted && !literal_host_is_public(&parsed) => errors.add(
            "url",
            "The address is not public: loopback, private and link-local addresses are refused.",
        ),
        Ok(_) => {}
        Err(_) => errors.add("url", INVALID_URL),
    }
    let mut checked = Vec::new();
    for value in &form.events {
        match value.parse::<EventType>() {
            Ok(event) => checked.push(event),
            Err(_) => errors.add("events", format!("Unknown event type `{value}`.")),
        }
    }
    let events = EventType::ALL
        .into_iter()
        .filter(|event| checked.contains(event))
        .collect();
    errors.into_result(SubscriptionInput {
        url: url.to_string(),
        events,
    })
}

fn base_path(key: &str) -> String {
    format!("/projects/{key}/subscriptions")
}

/// Where the editor posts and what it is called.
struct EditorTarget {
    /// The tab fragment, which Cancel fetches again.
    base: String,
    action: String,
    submit: &'static str,
}

impl EditorTarget {
    fn new(key: &str) -> Self {
        let base = base_path(key);
        Self {
            action: base.clone(),
            base,
            submit: "Create subscription",
        }
    }

    fn existing(key: &str, id: Uuid) -> Self {
        let base = base_path(key);
        Self {
            action: format!("{base}/{id}"),
            base,
            submit: "Save subscription",
        }
    }
}

/// The inline editor of a new or an existing subscription. Its inputs and the submit button
/// belong to [`form_anchor`] (`form` attribute), so Enter in the URL field submits the
/// subscription and "Save project" never sends these fields.
fn editor(
    target: &EditorTarget,
    values: &SubscriptionForm,
    errors: &FormErrors,
    targets: ProjectTargets,
) -> Markup {
    let hint = match targets {
        ProjectTargets::PublicHttps => "HTTPS only. The host must resolve to public addresses.",
        ProjectTargets::Unrestricted => "Any http or https URL (development build).",
    };
    html! {
        div id="subscription-editor" class="space-y-4 rounded-xl border border-line-strong bg-panel p-4" {
            (form_error(errors))
            (mono_text_field_of_form("Receiver URL", "url", &values.url, errors, Some(hint), FORM_ID))
            (events_field(&values.events, errors))
            div class="flex justify-end gap-2" {
                button type="button" hx-get=(target.base) hx-target="#lifecycle-events" hx-swap="outerHTML"
                    class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                button type="submit" form=(FORM_ID) hx-post=(target.action) hx-include={"#" (FORM_ID)}
                    hx-target="#subscription-editor" hx-swap="outerHTML"
                    class=(action_class(ButtonKind::Primary)) { (target.submit) }
            }
        }
    }
}

fn events_field(selected: &[String], errors: &FormErrors) -> Markup {
    html! {
        fieldset class="space-y-1" aria-describedby="events-hint" {
            legend class="block text-sm font-medium text-muted" { "Event types" }
            div class="grid grid-cols-1 md:grid-cols-2" {
                @for event in EventType::ALL {
                    label class="flex min-h-11 items-center gap-2 lg:min-h-9" {
                        input type="checkbox" name="events" value=(event.as_str()) form=(FORM_ID)
                            checked[selected.iter().any(|s| s == event.as_str())];
                        span class="font-mono text-[13px]" { (event.as_str()) }
                    }
                }
            }
            p id="events-hint" class="text-xs text-faint" {
                "None checked: all event types, including types added later."
            }
            (field_error(errors.get("events")))
        }
    }
}

/// The instance public key receivers verify signatures with, or a note that the feature is off;
/// a warning when the URL restrictions are off (development build).
fn key_panel(public_key: Option<&str>, targets: ProjectTargets) -> Markup {
    html! {
        section class="rounded-xl border border-line bg-panel px-4 py-3 text-sm" {
            @if targets == ProjectTargets::Unrestricted {
                p class="mb-2 text-danger" {
                    "Receiver URLs are unrestricted (development build): plain http and internal "
                    "addresses are accepted."
                }
            }
            @if let Some(key) = public_key {
                p class="text-muted" {
                    "Receivers verify the " code class="font-mono text-[13px]" { "webhook-signature" }
                    " header with the public key of this instance:"
                }
                p class="mt-1 font-mono text-[13px] break-all text-ink" { (key) }
                a href=(PUBLIC_KEY_PATH) class="mt-1 inline-block text-accent-text underline" { "Public key endpoint" }
            } @else {
                p class="text-muted" {
                    "Lifecycle events are off on this instance: no signing key is configured. "
                    "Subscriptions are kept but receive nothing."
                }
            }
        }
    }
}

/// Time and HTTP status or error of the last delivery attempt.
fn last_delivery(subscription: &Subscription) -> Markup {
    html! {
        @if let Some(at) = subscription.last_attempt_at {
            time datetime=(at.to_rfc3339()) { (at.format("%b %d %H:%M UTC")) }
            " · "
            @if let Some(status) = subscription.last_status {
                @let failed = !StatusCode::from_u16(status).is_ok_and(|status| status.is_success());
                span class=[failed.then_some("text-danger")] { "HTTP " (status) }
            } @else if let Some(error) = &subscription.last_error {
                span class="text-danger" { (error) }
            }
        } @else {
            span class="text-faint" { "No delivery yet" }
        }
    }
}

fn events_label(events: &[EventType]) -> String {
    if events.is_empty() {
        "All event types".into()
    } else {
        events
            .iter()
            .map(EventType::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Which subscription editor the tab shows. The region renders at most one, so a second open
/// replaces the first and the editor ID and the `form`-bound inputs stay unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Closed,
    New,
    Edit(Uuid),
}

/// The tab content: intro, key panel, the subscriptions with their editor slot. Replaced as a
/// whole after every change, which also refreshes the count.
fn region(
    project: &ProjectRecord,
    subscriptions: &[Subscription],
    public_key: Option<&str>,
    targets: ProjectTargets,
    open: Open,
) -> Markup {
    let base = base_path(&project.key);
    let new = new_button(&base);
    let new_editor = html! {
        @if open == Open::New {
            (editor(&EditorTarget::new(&project.key), &SubscriptionForm::default(), &FormErrors::default(), targets))
        }
    };
    html! {
        div id="lifecycle-events" class="space-y-4" {
            p class="text-sm text-muted" {
                "Signed events about the volumes and sandboxes of this project, sent to each receiver. "
                "Subscriptions save on their own; Save project does not apply to them."
            }
            (key_panel(public_key, targets))
            @if subscriptions.is_empty() {
                (new_editor)
                (empty_panel(
                    "No subscriptions yet",
                    "A subscription sends signed events about the volumes and sandboxes of this project to a receiver URL.",
                    new,
                ))
            } @else {
                div class="flex items-center justify-between gap-2" {
                    h3 class="text-sm font-medium" {
                        "Subscriptions " span class="font-normal text-faint" { (subscriptions.len()) }
                    }
                    (new)
                }
                (new_editor)
                ul class="divide-y divide-line rounded-xl border border-line bg-panel" {
                    @for s in subscriptions {
                        @if open == Open::Edit(s.id) {
                            li {
                                (editor(&EditorTarget::existing(&project.key, s.id), &SubscriptionForm::of(s), &FormErrors::default(), targets))
                            }
                        } @else {
                            (row(&base, s))
                        }
                    }
                }
            }
        }
    }
}

fn new_button(base: &str) -> Markup {
    html! {
        button type="button" hx-get={(base) "/new"} hx-target="#lifecycle-events" hx-swap="outerHTML"
            class=(action_class(ButtonKind::Secondary)) {
            (icon(Icon::Plus)) "New subscription"
        }
    }
}

/// A subscription: URL, event types and last delivery with edit and delete actions. Delete asks
/// first (client state only); only the confirmation sends the request.
fn row(base: &str, s: &Subscription) -> Markup {
    html! {
        li x-data="{ confirming: false }" {
            div x-show="!confirming" class="flex items-center justify-between gap-3 px-4 py-3" {
                div class="min-w-0 space-y-0.5" {
                    p class="font-mono text-[13px] break-all" { (s.url) }
                    p class="text-xs text-muted" { (events_label(&s.events)) }
                    p class="text-xs text-muted" { (last_delivery(s)) }
                }
                span class="flex shrink-0" {
                    button type="button" hx-get={(base) "/" (s.id) "/edit"} hx-target="#lifecycle-events" hx-swap="outerHTML"
                        aria-label={"Edit subscription " (s.url)} title="Edit" class=(ICON_BUTTON_CLASS) {
                        (icon(Icon::Pencil))
                    }
                    button type="button" x-ref="trash" "x-on:click"="confirming = true; $nextTick(() => $refs.cancel.focus())"
                        aria-label={"Delete subscription " (s.url)} title="Delete" class=(ICON_BUTTON_CLASS) {
                        (icon(Icon::Trash))
                    }
                }
            }
            div x-show="confirming" x-cloak class="flex flex-wrap items-center justify-between gap-3 rounded-b-xl bg-danger-soft px-4 py-3" {
                div class="min-w-0 space-y-0.5" {
                    p class="font-mono text-[13px] break-all" { (s.url) }
                    p class="text-xs" { "Delete this subscription? Deliveries to it stop at once." }
                }
                span class="flex shrink-0 gap-2" {
                    button type="button" x-ref="cancel" "x-on:click"="confirming = false; $nextTick(() => $refs.trash.focus())" class=(action_class(ButtonKind::Secondary)) { "Cancel" }
                    button type="button" hx-delete={(base) "/" (s.id)} hx-target="#lifecycle-events" hx-swap="outerHTML"
                        class=(action_class(ButtonKind::Danger)) { "Delete" }
                }
            }
        }
    }
}

async fn project(state: &WebState, key: &str) -> Result<ProjectRecord, Box<Response>> {
    match df_db::catalog::get_project_by_key(&state.pool, key).await {
        Ok(Some(project)) => Ok(project),
        Ok(None) => Err(Box::new(not_found("The project"))),
        Err(e) => Err(Box::new(internal_error(&e))),
    }
}

/// The whole tab with `open` editor, or the error response when the list cannot be read.
async fn region_response(state: &WebState, project: &ProjectRecord, open: Open) -> Response {
    match event_subscriptions::list(&state.pool, project.id).await {
        Ok(subscriptions) => region(
            project,
            &subscriptions,
            state.lifecycle_public_key.as_deref(),
            state.project_targets,
            open,
        )
        .into_response(),
        Err(e) => internal_error(&e),
    }
}

/// Fragment of the tab. A plain browser request (the old page URL) goes to the project list.
async fn tab(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !is_htmx(&headers) {
        return Redirect::to("/projects").into_response();
    }
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    region_response(&state, &project, Open::Closed).await
}

async fn new_form(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !is_htmx(&headers) {
        return Redirect::to("/projects").into_response();
    }
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    region_response(&state, &project, Open::New).await
}

async fn edit_form(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path((key, id)): Path<(String, Uuid)>,
) -> Response {
    if !is_htmx(&headers) {
        return Redirect::to("/projects").into_response();
    }
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    match event_subscriptions::get(&state.pool, project.id, id).await {
        Ok(Some(_)) => region_response(&state, &project, Open::Edit(id)).await,
        Ok(None) => not_found("The subscription"),
        Err(e) => internal_error(&e),
    }
}

async fn create(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    RawForm(body): RawForm,
) -> Response {
    save(&state, &user, &key, None, &body).await
}

async fn update(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path((key, id)): Path<(String, Uuid)>,
    RawForm(body): RawForm,
) -> Response {
    save(&state, &user, &key, Some(id), &body).await
}

async fn save(
    state: &WebState,
    user: &CurrentUser,
    key: &str,
    id: Option<Uuid>,
    body: &[u8],
) -> Response {
    let project = match project(state, key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    let form = SubscriptionForm::parse(body);
    let input = match validate(&form, state.project_targets) {
        Ok(input) => input,
        Err(errors) => {
            let target = match id {
                Some(id) => EditorTarget::existing(&project.key, id),
                None => EditorTarget::new(&project.key),
            };
            return invalid(editor(&target, &form, &errors, state.project_targets));
        }
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match event_subscriptions::save(&state.pool, actor, project.id, id, &input).await {
        Ok(_) => retargeted(region_response(state, &project, Open::Closed).await),
        Err(DbError::NotFound(_)) => not_found("The subscription"),
        Err(e) => internal_error(&e),
    }
}

async fn remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path((key, id)): Path<(String, Uuid)>,
) -> Response {
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match event_subscriptions::delete(&state.pool, actor, project.id, id).await {
        Ok(()) => region_response(&state, &project, Open::Closed).await,
        Err(DbError::NotFound(_)) => not_found("The subscription"),
        Err(e) => internal_error(&e),
    }
}

/// A saved subscription: the whole region replaces the tab content, not just the editor the
/// request targeted.
fn retargeted(mut response: Response) -> Response {
    if !response.status().is_success() {
        return response;
    }
    let headers = response.headers_mut();
    headers.insert("HX-Retarget", HeaderValue::from_static("#lifecycle-events"));
    headers.insert("HX-Reswap", HeaderValue::from_static("outerHTML"));
    response
}

#[cfg(test)]
mod tests;
