//! Lifecycle event subscriptions of a project (`event-subscriptions` spec): a settings page for
//! users with the `admin` role (design D8), linked from the project list.
//!
//! The forms open as a modal over the page like the other settings; a plain browser request for
//! one goes to the page instead.

use axum::extract::{Path, RawForm, State};
use axum::http::{HeaderMap, StatusCode};
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
    Cell, FormErrors, ICON_BUTTON_CLASS, Icon, data_table, empty_panel, field_error, form_error,
    icon, td, text_field, th, tr,
};
use crate::config_pages::{
    create_button, edit_button, form_footer, list_header, list_page, modal, saved,
};
use crate::layout::{Nav, Shell};
use crate::responses::{internal_error, invalid, not_found};

/// The endpoint receivers fetch the instance public key from.
const PUBLIC_KEY_PATH: &str = "/lifecycle-events/public-key";
const INVALID_URL: &str = "Enter a valid URL, for example https://hooks.example.org/events.";

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/projects/{key}/subscriptions", get(page).post(create))
        .route("/projects/{key}/subscriptions/new", get(new_form))
        .route(
            "/projects/{key}/subscriptions/{id}",
            post(update).delete(remove),
        )
        .route("/projects/{key}/subscriptions/{id}/edit", get(edit_form))
        .route_layer(middleware::from_fn(require_admin))
}

/// Link of a project row to its subscriptions; the project list shows it to admins only.
pub(crate) fn settings_link(key: &str, class: &str) -> Markup {
    html! {
        a href=(base_path(key)) aria-label={"Lifecycle events of " (key)} title="Lifecycle events" class=(class) {
            (icon(Icon::Send))
        }
    }
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
/// [`ProjectTargets::Unrestricted`] any valid URL is accepted.
fn validate(
    form: &SubscriptionForm,
    targets: ProjectTargets,
) -> Result<SubscriptionInput, FormErrors> {
    let mut errors = FormErrors::default();
    let url = form.url.trim();
    let restricted = targets == ProjectTargets::PublicHttps;
    match Url::parse(url) {
        Ok(parsed) if parsed.host().is_none() => errors.add("url", INVALID_URL),
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

fn subscription_form(
    action: &str,
    values: &SubscriptionForm,
    errors: &FormErrors,
    submit: &str,
) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (text_field("Receiver URL", "url", &values.url, errors,
                Some("HTTPS only. The host must resolve to public addresses.")))
            (events_field(&values.events, errors))
            (form_footer(submit))
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
                        input type="checkbox" name="events" value=(event.as_str())
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

fn region(
    project: &ProjectRecord,
    subscriptions: &[Subscription],
    public_key: Option<&str>,
    targets: ProjectTargets,
) -> Markup {
    let base = base_path(&project.key);
    let new = create_button(&format!("{base}/new"), "New subscription");
    html! {
        div id="config-list" class="space-y-4" {
            (list_header(&format!("Lifecycle events of {}", project.name), subscriptions.len(), new.clone(), None))
            (key_panel(public_key, targets))
            @if subscriptions.is_empty() {
                (empty_panel(
                    "No subscriptions yet",
                    "A subscription sends signed events about the volumes and sandboxes of this project to a receiver URL.",
                    new,
                ))
            } @else {
                (data_table(
                    html! {
                        (th(Cell::First, html! { "Receiver URL" })) (th(Cell::Text, html! { "Events" }))
                        (th(Cell::Text, html! { "Last delivery" }))
                        (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                    },
                    html! {
                        @for s in subscriptions {
                            (tr(html! {
                                (td(Cell::First, html! { span class="font-mono text-[13px] break-all" { (s.url) } }))
                                (td(Cell::Text, html! { (events_label(&s.events)) }))
                                (td(Cell::Text, last_delivery(s)))
                                (td(Cell::Actions, html! {
                                    span class="flex justify-end" {
                                        (edit_button(&format!("{base}/{}/edit", s.id), &format!("Edit subscription {}", s.url)))
                                        button type="button" hx-delete={(base) "/" (s.id)}
                                            hx-target="#config-list" hx-swap="outerHTML"
                                            aria-label={"Delete subscription " (s.url)} title="Delete" class=(ICON_BUTTON_CLASS) {
                                            (icon(Icon::Trash))
                                        }
                                    }
                                }))
                            }))
                        }
                    },
                ))
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

async fn region_markup(state: &WebState, project: &ProjectRecord) -> Result<Markup, Box<Response>> {
    let subscriptions = event_subscriptions::list(&state.pool, project.id)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(region(
        project,
        &subscriptions,
        state.lifecycle_public_key.as_deref(),
        state.project_targets,
    ))
}

async fn page(State(state): State<WebState>, shell: Shell, Path(key): Path<String>) -> Response {
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    match region_markup(&state, &project).await {
        Ok(region) => list_page(
            &format!("Lifecycle events · {}", project.key),
            Nav::Projects,
            &shell,
            region,
        ),
        Err(response) => *response,
    }
}

async fn new_form(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(key): Path<String>,
) -> Response {
    if !is_htmx(&headers) {
        return Redirect::to(&base_path(&key)).into_response();
    }
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    modal(
        "New subscription",
        subscription_form(
            &base_path(&project.key),
            &SubscriptionForm::default(),
            &FormErrors::default(),
            "Create subscription",
        ),
    )
}

async fn edit_form(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path((key, id)): Path<(String, Uuid)>,
) -> Response {
    if !is_htmx(&headers) {
        return Redirect::to(&base_path(&key)).into_response();
    }
    let project = match project(&state, &key).await {
        Ok(project) => project,
        Err(response) => return *response,
    };
    match event_subscriptions::get(&state.pool, project.id, id).await {
        Ok(Some(subscription)) => modal(
            "Edit subscription",
            subscription_form(
                &format!("{}/{id}", base_path(&project.key)),
                &SubscriptionForm::of(&subscription),
                &FormErrors::default(),
                "Save subscription",
            ),
        ),
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
            let base = base_path(&project.key);
            let (action, submit) = match id {
                Some(id) => (format!("{base}/{id}"), "Save subscription"),
                None => (base, "Create subscription"),
            };
            return invalid(subscription_form(&action, &form, &errors, submit));
        }
    };
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match event_subscriptions::save(&state.pool, actor, project.id, id, &input).await {
        Ok(_) => match region_markup(state, &project).await {
            Ok(region) => saved(region),
            Err(response) => *response,
        },
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
        Ok(()) => match region_markup(&state, &project).await {
            Ok(region) => region.into_response(),
            Err(response) => *response,
        },
        Err(DbError::NotFound(_)) => not_found("The subscription"),
        Err(e) => internal_error(&e),
    }
}

#[cfg(test)]
mod tests;
