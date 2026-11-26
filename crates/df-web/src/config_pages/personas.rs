//! Persona pages (`specs/configuration-management/spec.md`): identity (display name, handle,
//! commit email, avatar), harness, model provider and the model of that provider, both
//! compatible with the harness, and the system prompt. The list and the "New persona" modal are
//! here, the settings page of a persona is [`super::persona_page`].

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use df_core::PERSONA_DESCRIPTION_MAX_CHARS;
use df_core::harness;
use df_core::harness::PROFILES;
use df_core::persona::{parse_display_name, parse_email, parse_handle};
use df_db::DbError;
use df_db::catalog::{Actor, PersonaField, PersonaInput, PersonaRecord};
use df_db::model_providers::ModelProviderRecord;
use df_db::persona_keys::sync_signing_key;
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::signing_key::{SigningKeySection, commits_group};
use super::{
    conflict, create_button, form_footer, form_grid, full_row, list_header, list_page, modal,
    persona_page::{self, PersonaTab},
};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::{INPUT_CLASS, field, form_group, select_field_with_hint};
use crate::components::{
    AvatarSize, FormErrors, ICON_BUTTON_CLASS, Icon, empty_panel, form_error, harness_label, icon,
    persona_avatar, persona_avatar_preview, text_field,
};
use crate::layout::{Nav, Shell};
use crate::responses::{htmx_redirect, internal_error, invalid, not_found};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/personas", get(personas_list).post(persona_create))
        .route("/personas/new", get(persona_new))
        .route("/personas/refresh", post(persona_refresh))
        .route(
            "/personas/{id}",
            get(persona_page::page).post(persona_update),
        )
        .route(
            "/personas/{id}/rotate-key",
            post(super::signing_key::rotate),
        )
        .route(
            "/personas/{id}/verify-key",
            post(super::signing_key::verify_forgejo),
        )
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct PersonaForm {
    pub(super) display_name: String,
    handle: String,
    pub(super) commit_email: String,
    description: String,
    pub(super) system_prompt: String,
    harness: String,
    model_provider_id: String,
    model: String,
    /// The tab the settings page showed when it was saved; it stays open afterwards.
    pub(super) tab: PersonaTab,
}

impl PersonaForm {
    /// The harness of the form; the first while none is chosen (what a browser selects).
    fn harness(&self) -> &str {
        harness::profile(&self.harness).unwrap_or(&PROFILES[0]).name
    }

    fn provider<'a>(
        &self,
        providers: &'a [ModelProviderRecord],
    ) -> Option<&'a ModelProviderRecord> {
        let id = Uuid::parse_str(&self.model_provider_id).ok()?;
        providers.iter().find(|provider| provider.id == id)
    }

    /// Drops the provider and the model that the harness cannot use, like the selects offer.
    fn normalized(mut self, providers: &[ModelProviderRecord]) -> Self {
        self.harness = self.harness().to_string();
        let provider = self.provider(providers).filter(|provider| {
            harness::check_provider_compatible(
                &self.harness,
                &provider.wire_apis(),
                provider.input.scheme,
            )
            .is_ok()
        });
        let models = model_options(provider, &self.harness);
        if provider.is_none() {
            self.model_provider_id.clear();
        }
        if !models.iter().any(|(id, _)| *id == self.model) {
            self.model.clear();
        }
        self
    }
}

fn validate_persona(
    form: &PersonaForm,
    providers: &[ModelProviderRecord],
) -> Result<PersonaInput, FormErrors> {
    let mut errors = FormErrors::default();
    let display_name = identity_part(
        &mut errors,
        "display_name",
        "Display name",
        &form.display_name,
        parse_display_name,
    );
    let handle = identity_part(&mut errors, "handle", "Handle", &form.handle, parse_handle);
    let commit_email = identity_part(
        &mut errors,
        "commit_email",
        "Commit email",
        &form.commit_email,
        parse_email,
    );
    let description = form.description.trim().to_string();
    if description.chars().count() > PERSONA_DESCRIPTION_MAX_CHARS {
        errors.add(
            "description",
            format!("Description is at most {PERSONA_DESCRIPTION_MAX_CHARS} characters."),
        );
    }
    let harness_name = form.harness();
    let provider = form.provider(providers);
    match provider {
        None => errors.add("model_provider_id", "Choose a model provider."),
        Some(provider) => {
            if let Err(error) = harness::check_provider_compatible(
                harness_name,
                &provider.wire_apis(),
                provider.input.scheme,
            ) {
                errors.add("model_provider_id", error.to_string());
            }
        }
    }
    let model = form.model.trim();
    if model.is_empty() {
        errors.add("model", "Model is required.");
    } else if let Some(provider) = provider {
        match provider.model(model) {
            None => errors.add("model", "Choose a model of the model provider."),
            Some(found) => {
                if let Err(error) =
                    harness::check_compatible(harness_name, found.wire_api, provider.input.scheme)
                {
                    errors.add("model", error.to_string());
                }
            }
        }
    }
    errors.into_result(PersonaInput {
        handle,
        display_name,
        commit_email,
        description,
        // Optional, stored as typed: a browser submits a textarea's line breaks as CRLF.
        system_prompt: form.system_prompt.replace("\r\n", "\n"),
        harness: harness_name.to_string(),
        model: model.to_string(),
        model_provider_id: provider.map_or(Uuid::nil(), |provider| provider.id),
    })
}

/// One part of the identity: `parse` it, or put its error (or "is required") at the field.
fn identity_part(
    errors: &mut FormErrors,
    field: &'static str,
    label: &str,
    value: &str,
    parse: fn(&str) -> Result<String, df_core::IdentityError>,
) -> String {
    if value.trim().is_empty() {
        errors.add(field, format!("{label} is required."));
        return String::new();
    }
    match parse(value.trim()) {
        Ok(parsed) => parsed,
        Err(error) => {
            errors.add(field, error.to_string());
            value.trim().to_string()
        }
    }
}

/// The model options for the persona form: the models of `provider` the harness can use, with
/// their wire API.
fn model_options(
    provider: Option<&ModelProviderRecord>,
    harness_name: &str,
) -> Vec<(String, String)> {
    let Some(provider) = provider else {
        return Vec::new();
    };
    provider
        .input
        .models
        .iter()
        .filter(|model| {
            harness::check_compatible(harness_name, model.wire_api, provider.input.scheme).is_ok()
        })
        .map(|model| {
            (
                model.model_id.clone(),
                format!("{} ({})", model.model_id, model.wire_api.label()),
            )
        })
        .collect()
}

pub(super) fn persona_values(p: &PersonaRecord) -> PersonaForm {
    PersonaForm {
        display_name: p.display_name.clone(),
        handle: p.handle.clone(),
        commit_email: p.commit_email.clone(),
        description: p.description.clone(),
        system_prompt: p.system_prompt.clone(),
        harness: p.harness.clone(),
        model: p.model.clone(),
        model_provider_id: p.model_provider_id.to_string(),
        tab: PersonaTab::default(),
    }
}

/// A select whose change renders [`agent_fields`] again, so the lists below follow.
fn refreshing_select(
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    hint: Option<&str>,
    errors: &FormErrors,
) -> Markup {
    field(
        label,
        name,
        errors,
        hint,
        html! {
            select id=(name) name=(name) class=(INPUT_CLASS)
                hx-post="/personas/refresh" hx-trigger="change" hx-target="#agent-fields" hx-swap="outerHTML"
                aria-invalid=[errors.get(name).map(|_| "true")] {
                @for (value, text) in options { option value=(value) selected[value == selected] { (text) } }
            }
        },
    )
}

/// The "New persona" form in the groups "Identity", "Agent" and "Commits" (no system prompt: the
/// settings page of the new persona opens on it). The group "Commits" holds the commit email and
/// the note that the key comes with the first save.
fn create_form(
    values: &PersonaForm,
    errors: &FormErrors,
    providers: &[ModelProviderRecord],
) -> Markup {
    html! {
        form id="form" class="space-y-4" hx-post="/personas" hx-target="#form" hx-swap="outerHTML"
            x-data={"personaAvatar(" (serde_json::json!(values.display_name)) ")"} "x-on:input"="track($event)" {
            (form_error(errors))
            div class="space-y-5" {
                (form_group("Identity", identity_fields(None, values, errors)))
                (form_group("Agent", agent_fields(values, errors, providers)))
                (commits_group(&values.commit_email, errors, &SigningKeySection::default()))
            }
            (form_footer("Create persona"))
        }
    }
}

/// Harness, model provider and model: `#agent-fields`, which a change of the harness or provider
/// renders again.
pub(super) fn agent_fields(
    values: &PersonaForm,
    errors: &FormErrors,
    providers: &[ModelProviderRecord],
) -> Markup {
    let harness_name = values.harness();
    let harnesses: Vec<(String, String)> = PROFILES
        .iter()
        .map(|profile| (profile.name.to_string(), profile.name.to_string()))
        .collect();
    let models = model_options(values.provider(providers), harness_name);
    html! {
        div id="agent-fields" {
            (form_grid(html! {
                (refreshing_select("Harness", "harness", &harnesses, harness_name, None, errors))
                (refreshing_select("Model provider", "model_provider_id", &with_empty(super::providers::compatible_options(providers, harness_name)),
                    &values.model_provider_id, Some("Only providers that offer a wire API of the harness."), errors))
                (full_row(select_field_with_hint("Model", "model", &models, &values.model, Some("Choose…"),
                    Some("Required. Only models of the provider whose wire API the harness can use."), errors)))
            }))
        }
    }
}

/// The avatar preview left of display name and handle, and the description below. The preview
/// is neutral until the persona is saved.
pub(super) fn identity_fields(
    id: Option<Uuid>,
    values: &PersonaForm,
    errors: &FormErrors,
) -> Markup {
    html! {
        div class="space-y-4" {
            div class="flex items-start gap-4" {
                // 24 px: the label above the inputs, so the avatar sits beside the first input.
                div class="pt-6" { (persona_avatar_preview(id, &values.display_name, AvatarSize::Form)) }
                div class="grid min-w-0 flex-1 gap-4 sm:grid-cols-2" {
                    (text_field("Display name", "display_name", &values.display_name, errors, None))
                    (handle_field(&values.handle, errors))
                }
            }
            (description_field(&values.description, errors))
        }
    }
}

/// The handle input with the `@` in front of it.
fn handle_field(value: &str, errors: &FormErrors) -> Markup {
    let class = format!("{INPUT_CLASS} pl-7 font-mono");
    field(
        "Handle",
        "handle",
        errors,
        None,
        html! {
            div class="relative" {
                span class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" aria-hidden="true" { "@" }
                input id="handle" name="handle" type="text" value=(value) class=(class)
                    autocapitalize="none" autocomplete="off" spellcheck="false"
                    aria-invalid=[errors.get("handle").map(|_| "true")]
                    aria-describedby=[errors.get("handle").map(|_| "handle-error")];
            }
        },
    )
}

/// The description textarea with its `n / 500` counter, which follows what is typed.
fn description_field(value: &str, errors: &FormErrors) -> Markup {
    let count = value.chars().count();
    field(
        "Description",
        "description",
        errors,
        Some("Role and strengths. Agents read it when they choose whom to delegate to."),
        html! {
            div x-data={"{ n: " (count) " }"} {
                textarea id="description" name="description" rows="3" class=(INPUT_CLASS)
                    "x-on:input"="n = $event.target.value.length"
                    aria-invalid=[errors.get("description").map(|_| "true")] { (value) }
                p class="mt-1 text-right text-xs text-faint" {
                    span x-text="n" { (count) } " / " (PERSONA_DESCRIPTION_MAX_CHARS)
                }
            }
        },
    )
}

/// `options` with the empty choice in front.
fn with_empty(mut options: Vec<(String, String)>) -> Vec<(String, String)> {
    options.insert(0, (String::new(), "Choose…".to_string()));
    options
}

fn personas_region(personas: &[PersonaRecord], providers: &[ModelProviderRecord]) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Personas", personas.len(), create_button("/personas/new", "New persona"), None))
            @if personas.is_empty() {
                (empty_panel(
                    "No personas yet",
                    "A persona is the agent a task is assigned to: a system prompt, a harness and a model.",
                    create_button("/personas/new", "New persona"),
                ))
            } @else {
                ul class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3" {
                    @for p in personas {
                        @let provider = providers.iter().find(|provider| provider.id == p.model_provider_id)
                            .map(|provider| provider.input.name.as_str());
                        li class="flex min-w-0 flex-col gap-3 rounded-xl border border-line bg-panel p-4" {
                            div class="flex items-start gap-3" {
                                (persona_avatar(Some(p.id), &p.display_name, AvatarSize::Card))
                                div class="min-w-0 flex-1" {
                                    h2 class="truncate font-semibold" {
                                        a href={"/personas/" (p.id)} class="hover:underline" { (p.display_name) }
                                        " " span class="font-normal text-muted" { "@" (p.handle) }
                                    }
                                    p class="truncate text-xs text-muted" { (p.commit_email) }
                                    p class="text-xs text-muted" { (harness_label(&p.harness)) " · " (p.model) }
                                }
                                a href={"/personas/" (p.id)} aria-label={"Edit persona " (p.display_name)}
                                    title={"Edit persona " (p.display_name)} class=(ICON_BUTTON_CLASS) { (icon(Icon::Pencil)) }
                            }
                            @if p.description.is_empty() && p.system_prompt.is_empty() {
                                p class="text-sm text-faint" { "No system prompt yet" }
                            } @else {
                                p class="line-clamp-3 whitespace-pre-wrap break-words text-sm text-muted" {
                                    @if p.description.is_empty() { (p.system_prompt) } @else { (p.description) }
                                }
                            }
                            p class="mt-auto text-xs text-faint" {
                                "Model provider: " span class="font-mono" { (provider.unwrap_or("none")) }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn personas_markup(state: &WebState) -> Result<Markup, Box<Response>> {
    let (personas, providers) = tokio::try_join!(
        df_db::catalog::list_personas(&state.pool),
        df_db::model_providers::list_model_providers(&state.pool)
    )
    .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(personas_region(&personas, &providers))
}

async fn personas_list(State(state): State<WebState>, shell: Shell) -> Response {
    match personas_markup(&state).await {
        Ok(region) => list_page("Personas", Nav::Personas, &shell, region),
        Err(response) => *response,
    }
}

async fn persona_new(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/personas").into_response();
    }
    let providers = match df_db::model_providers::list_model_providers(&state.pool).await {
        Ok(providers) => providers,
        Err(e) => return internal_error(&e),
    };
    modal(
        "New persona",
        create_form(&PersonaForm::default(), &FormErrors::default(), &providers),
    )
}

/// A change of harness or model provider: the group "Agent" as typed, rendered again with the
/// provider and model lists of the new choice. Nothing else of the form changes.
async fn persona_refresh(State(state): State<WebState>, Form(form): Form<PersonaForm>) -> Response {
    let providers = match df_db::model_providers::list_model_providers(&state.pool).await {
        Ok(providers) => providers,
        Err(e) => return internal_error(&e),
    };
    let form = form.normalized(&providers);
    agent_fields(&form, &FormErrors::default(), &providers).into_response()
}

async fn persona_create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<PersonaForm>,
) -> Response {
    save_persona(state, user, None, form.0).await
}

async fn persona_update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    form: Form<PersonaForm>,
) -> Response {
    save_persona(state, user, Some(id), form.0).await
}

async fn save_persona(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    id: Option<Uuid>,
    form: PersonaForm,
) -> Response {
    let providers = match df_db::model_providers::list_model_providers(&state.pool).await {
        Ok(providers) => providers,
        Err(e) => return internal_error(&e),
    };
    let errors = match validate_persona(&form, &providers) {
        Ok(input) => {
            let principal = user.principal();
            let actor = Actor {
                principal: &principal,
                roles: &user.roles,
            };
            match df_db::catalog::save_persona(&state.pool, actor, id, &input).await {
                Ok(persona) => {
                    ensure_signing_key(&state, persona.id).await;
                    // A new persona gets its prompt written next, with the full editor.
                    if id.is_none() {
                        return htmx_redirect(&format!(
                            "/personas/{}?tab=system-prompt",
                            persona.id
                        ));
                    }
                    return persona_page::saved(&state, persona.id, &providers, form.tab).await;
                }
                Err(DbError::NotFound(_)) => return not_found("The persona"),
                Err(e) => match save_error(e) {
                    Ok(errors) => errors,
                    Err(response) => return *response,
                },
            }
        }
        Err(errors) => errors,
    };
    let Some(id) = id else {
        return invalid(create_form(&form, &errors, &providers));
    };
    match persona_page::form_with_errors(&state, id, &form, &errors, &providers).await {
        Ok(page_form) => invalid(page_form),
        Err(response) => *response,
    }
}

/// Creates the signing key of a saved persona, or makes the columns agree with its Secret
/// (`sync_signing_key`). A failure is logged and does not undo the save: the key is created
/// before the persona's first run at the latest.
async fn ensure_signing_key(state: &WebState, id: Uuid) {
    if let Err(error) = sync_signing_key(&state.pool, state.secrets.as_ref(), id).await {
        tracing::error!(%error, persona = %id, "creating the signing key of a saved persona failed");
    }
}

/// The form errors of a failed save: a model conflict belongs to the model field, a taken handle
/// or email to its field (naming the persona that holds it), any other conflict (a concurrent
/// save) to the handle; other errors are an error page.
pub(super) fn save_error(error: DbError) -> Result<FormErrors, Box<Response>> {
    let mut errors = FormErrors::default();
    match error {
        DbError::ModelConflict(message) => errors.add("model", message),
        DbError::PersonaFieldTaken { field, holder } => {
            let name = match field {
                PersonaField::Handle => "handle",
                PersonaField::CommitEmail => "commit_email",
            };
            errors.add(name, format!("Already used by @{holder}."));
        }
        // The form checks every part first; this is the same rule failing in the database layer.
        DbError::InvalidPersonaIdentity(error) => {
            let name = match error {
                df_core::IdentityError::Handle => "handle",
                df_core::IdentityError::DisplayName => "display_name",
                df_core::IdentityError::Email => "commit_email",
            };
            errors.add(name, error.to_string());
        }
        other => {
            if let Some(response) = conflict(&mut errors, "handle", other) {
                return Err(Box::new(response));
            }
        }
    }
    Ok(errors)
}
