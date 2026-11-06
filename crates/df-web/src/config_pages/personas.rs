//! Persona pages (`specs/configuration-management/spec.md`): name, system prompt, harness, model
//! provider and the model of that provider, both compatible with the harness.

use axum::extract::{Form, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use df_core::PERSONA_DESCRIPTION_MAX_CHARS;
use df_core::harness;
use df_core::harness::PROFILES;
use df_db::DbError;
use df_db::catalog::{Actor, PersonaInput, PersonaRecord};
use df_db::model_providers::ModelProviderRecord;
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    FormTarget, conflict, create_button, edit_button, form_footer, form_grid, full_row,
    list_header, list_page, modal, required, saved,
};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::{INPUT_CLASS, field, select_field_with_hint};
use crate::components::{
    FormErrors, avatar, empty_panel, form_error, harness_label, initials, text_field,
    textarea_field,
};
use crate::layout::{Nav, Shell};
use crate::responses::{internal_error, invalid, not_found};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/personas", get(personas_list).post(persona_create))
        .route("/personas/new", get(persona_new))
        .route("/personas/refresh", post(persona_refresh))
        .route("/personas/{id}", post(persona_update))
        .route("/personas/{id}/edit", get(persona_edit))
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct PersonaForm {
    name: String,
    description: String,
    system_prompt: String,
    harness: String,
    model_provider_id: String,
    model: String,
    /// The persona being edited; empty when creating.
    entity_id: String,
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
    let name = required(&mut errors, "name", &form.name, "Name");
    let description = form.description.trim().to_string();
    if description.chars().count() > PERSONA_DESCRIPTION_MAX_CHARS {
        errors.add(
            "description",
            format!("Description is at most {PERSONA_DESCRIPTION_MAX_CHARS} characters."),
        );
    }
    let system_prompt = required(
        &mut errors,
        "system_prompt",
        &form.system_prompt,
        "System prompt",
    );
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
        name,
        description,
        system_prompt,
        harness: harness_name.to_string(),
        model: model.to_string(),
        model_provider_id: provider.map_or(Uuid::nil(), |provider| provider.id),
    })
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

fn persona_values(p: &PersonaRecord) -> PersonaForm {
    PersonaForm {
        name: p.name.clone(),
        description: p.description.clone(),
        system_prompt: p.system_prompt.clone(),
        harness: p.harness.clone(),
        model: p.model.clone(),
        model_provider_id: p.model_provider_id.to_string(),
        entity_id: String::new(),
    }
}

/// A select whose change renders the form again, so the lists below follow.
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
                hx-post="/personas/refresh" hx-trigger="change" hx-target="#form" hx-swap="outerHTML"
                aria-invalid=[errors.get(name).map(|_| "true")] {
                @for (value, text) in options { option value=(value) selected[value == selected] { (text) } }
            }
        },
    )
}

fn persona_form(
    target: FormTarget,
    values: &PersonaForm,
    errors: &FormErrors,
    providers: &[ModelProviderRecord],
    submit: &str,
) -> Markup {
    let harness_name = values.harness();
    let harnesses: Vec<(String, String)> = PROFILES
        .iter()
        .map(|profile| (profile.name.to_string(), profile.name.to_string()))
        .collect();
    let provider = values.provider(providers);
    let models = model_options(provider, harness_name);
    html! {
        form id="form" class="space-y-4" hx-post=(target.action()) hx-target="#form" hx-swap="outerHTML" {
            (target.id_field())
            (form_error(errors))
            (form_grid(html! {
                (full_row(text_field("Name", "name", &values.name, errors, None)))
                (full_row(description_field(&values.description, errors)))
                (full_row(textarea_field("System prompt", "system_prompt", &values.system_prompt, 8, errors)))
                (refreshing_select("Harness", "harness", &harnesses, harness_name, None, errors))
                (refreshing_select("Model provider", "model_provider_id", &with_empty(super::providers::compatible_options(providers, harness_name)),
                    &values.model_provider_id, Some("Only providers that offer a wire API of the harness."), errors))
                (full_row(select_field_with_hint("Model", "model", &models, &values.model, Some("Choose…"),
                    Some("Required. Only models of the provider whose wire API the harness can use."), errors)))
            }))
            (form_footer(submit))
        }
    }
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
                        li class="flex flex-col gap-3 rounded-xl border border-line bg-panel p-4" {
                            div class="flex items-start gap-3" {
                                (avatar(&initials(&p.name)))
                                div class="min-w-0 flex-1" {
                                    h2 class="truncate font-semibold" { (p.name) }
                                    p class="text-xs text-muted" { (harness_label(&p.harness)) " · " (p.model) }
                                }
                                (edit_button(&format!("/personas/{}/edit", p.id), &format!("Edit persona {}", p.name)))
                            }
                            p class="line-clamp-3 whitespace-pre-wrap break-words text-sm text-muted" {
                                @if p.description.is_empty() { (p.system_prompt) } @else { (p.description) }
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
        persona_form(
            FormTarget::new("/personas", None),
            &PersonaForm::default(),
            &FormErrors::default(),
            &providers,
            "Create persona",
        ),
    )
}

async fn persona_edit(
    State(state): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/personas").into_response();
    }
    let (persona, providers) = match tokio::try_join!(
        df_db::catalog::get_persona(&state.pool, id),
        df_db::model_providers::list_model_providers(&state.pool)
    ) {
        Ok((Some(persona), providers)) => (persona, providers),
        Ok((None, _)) => return not_found("The persona"),
        Err(e) => return internal_error(&e),
    };
    modal(
        "Edit persona",
        persona_form(
            FormTarget::new("/personas", Some(id)),
            &persona_values(&persona),
            &FormErrors::default(),
            &providers,
            "Save persona",
        ),
    )
}

/// A change of harness or model provider: the form as typed, rendered again with the provider
/// and model lists of the new choice.
async fn persona_refresh(State(state): State<WebState>, Form(form): Form<PersonaForm>) -> Response {
    let providers = match df_db::model_providers::list_model_providers(&state.pool).await {
        Ok(providers) => providers,
        Err(e) => return internal_error(&e),
    };
    let form = form.normalized(&providers);
    let target = FormTarget::posted("/personas", &form.entity_id);
    persona_form(
        target,
        &form,
        &FormErrors::default(),
        &providers,
        submit_label(target),
    )
    .into_response()
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
                Ok(_) => {
                    return match personas_markup(&state).await {
                        Ok(region) => saved(region),
                        Err(response) => *response,
                    };
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
    let target = FormTarget::new("/personas", id);
    invalid(persona_form(
        target,
        &form,
        &errors,
        &providers,
        submit_label(target),
    ))
}

/// The form errors of a failed save: a model conflict belongs to the model field, any other
/// conflict (a duplicate name) to the name; other errors are an error page.
pub(super) fn save_error(error: DbError) -> Result<FormErrors, Box<Response>> {
    let mut errors = FormErrors::default();
    match error {
        DbError::ModelConflict(message) => errors.add("model", message),
        other => {
            if let Some(response) = conflict(&mut errors, "name", other) {
                return Err(Box::new(response));
            }
        }
    }
    Ok(errors)
}

fn submit_label(target: FormTarget) -> &'static str {
    if target.is_create() {
        "Create persona"
    } else {
        "Save persona"
    }
}
