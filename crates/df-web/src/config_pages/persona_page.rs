//! The settings page of a persona, `/personas/{id}` (`configuration-management`, "Persona
//! settings page"): a back link, the persona's heading, the tabs General, System prompt and Commits
//! and one "Save persona".
//!
//! Like the project page, a save swaps only the form and, out of band, the heading and the status
//! line of the header. The System prompt tab holds the Markdown field with the split layout.

use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use df_db::catalog::PersonaRecord;
use df_db::model_providers::ModelProviderRecord;
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::personas::{PersonaForm, agent_fields, identity_fields, persona_values};
use super::signing_key::{Rotation, SigningKeySection, commits_group};
use super::{SettingsBack, Tab, TabsStyle, saved_response, settings_page_frame, tabbed_form};
use crate::WebState;
use crate::components::{
    AvatarSize, FormErrors, MarkdownField, Preview, form_group, harness_label, markdown_field,
    persona_avatar,
};
use crate::layout::{Nav, Shell, layout};
use crate::responses::{internal_error, not_found};

/// The tabs of the page, named in the URL (`?tab=system-prompt`) and posted as `tab`. An unknown
/// name is General, the tab a URL without `tab` opens.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub(super) enum PersonaTab {
    #[default]
    General,
    SystemPrompt,
    Commits,
}

impl From<String> for PersonaTab {
    fn from(id: String) -> Self {
        Self::ALL
            .into_iter()
            .find(|tab| tab.id() == id)
            .unwrap_or_default()
    }
}

impl PersonaTab {
    const ALL: [Self; 3] = [Self::General, Self::SystemPrompt, Self::Commits];

    fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::SystemPrompt => "system-prompt",
            Self::Commits => "commits",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::SystemPrompt => "System prompt",
            Self::Commits => "Commits",
        }
    }

    /// The tab that holds the form field `field`.
    fn of_field(field: &str) -> Self {
        match field {
            "system_prompt" => Self::SystemPrompt,
            "commit_email" => Self::Commits,
            _ => Self::General,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct PageQuery {
    tab: PersonaTab,
}

/// `GET /personas/{id}?tab=`: the page. An unknown `tab` opens General.
pub(super) async fn page(
    State(state): State<WebState>,
    shell: Shell,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Response {
    let (persona, providers) = match load(&state, id).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let signing =
        match SigningKeySection::load_or_error_page(&state, Some(id), Rotation::None).await {
            Ok(signing) => signing,
            Err(response) => return *response,
        };
    let form = page_form(
        id,
        &persona_values(&persona),
        &FormErrors::default(),
        &providers,
        &signing,
        query.tab,
    );
    let body = settings_page_frame(
        "persona-settings",
        SettingsBack {
            href: "/personas",
            label: "Personas",
        },
        heading(&persona),
        "Save persona",
        form,
    );
    layout(&persona.display_name, Nav::PersonaSettings, &shell, body).into_response()
}

/// The answer of a successful save: the form with the stored values and, out of band, the heading
/// (the name may have changed) and the "Persona saved" status. The tab stays open.
pub(super) async fn saved(
    state: &WebState,
    id: Uuid,
    providers: &[ModelProviderRecord],
    tab: PersonaTab,
) -> Response {
    let persona = match load_persona(state, id).await {
        Ok(persona) => persona,
        Err(response) => return *response,
    };
    let signing = match SigningKeySection::load_or_error_page(state, Some(id), Rotation::None).await
    {
        Ok(signing) => signing,
        Err(response) => return *response,
    };
    saved_response(
        page_form(
            id,
            &persona_values(&persona),
            &FormErrors::default(),
            providers,
            &signing,
            tab,
        ),
        "persona-heading",
        heading(&persona),
        "Persona saved",
    )
}

/// The form as typed with its `errors`, opened on the tab of the first error.
pub(super) async fn form_with_errors(
    state: &WebState,
    id: Uuid,
    values: &PersonaForm,
    errors: &FormErrors,
    providers: &[ModelProviderRecord],
) -> Result<Markup, Box<Response>> {
    let signing = SigningKeySection::load_or_error_page(state, Some(id), Rotation::None).await?;
    Ok(page_form(
        id, values, errors, providers, &signing, values.tab,
    ))
}

async fn load_persona(state: &WebState, id: Uuid) -> Result<PersonaRecord, Box<Response>> {
    match df_db::catalog::get_persona(&state.pool, id).await {
        Ok(Some(persona)) => Ok(persona),
        Ok(None) => Err(Box::new(not_found("The persona"))),
        Err(e) => Err(Box::new(internal_error(&e))),
    }
}

async fn load(
    state: &WebState,
    id: Uuid,
) -> Result<(PersonaRecord, Vec<ModelProviderRecord>), Box<Response>> {
    let providers = df_db::model_providers::list_model_providers(&state.pool)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok((load_persona(state, id).await?, providers))
}

/// The form with its three tabs. `requested` is the tab to open (the URL's or the posted one); a
/// form with errors opens the tab of its first error instead.
fn page_form(
    id: Uuid,
    values: &PersonaForm,
    errors: &FormErrors,
    providers: &[ModelProviderRecord],
    signing: &SigningKeySection,
    requested: PersonaTab,
) -> Markup {
    let tabs = [
        Tab {
            id: PersonaTab::General.id(),
            label: PersonaTab::General.label(),
            saves: true,
            panel: html! {
                div class="space-y-6" x-data={"personaAvatar(" (serde_json::json!(values.display_name)) ")"} "x-on:input"="track($event)" {
                    (form_group("Identity", identity_fields(Some(id), values, errors)))
                    (form_group("Agent", agent_fields(values, errors, providers)))
                }
            },
        },
        Tab {
            id: PersonaTab::SystemPrompt.id(),
            label: PersonaTab::SystemPrompt.label(),
            saves: true,
            panel: markdown_field(
                &MarkdownField {
                    label: "System prompt",
                    name: "system_prompt",
                    submit_name: None,
                    value: &values.system_prompt,
                    hint: None,
                    rows: 20,
                    preview: Preview::Split,
                    label_hidden: true,
                },
                errors,
            ),
        },
        Tab {
            id: PersonaTab::Commits.id(),
            label: PersonaTab::Commits.label(),
            saves: true,
            panel: commits_group(&values.commit_email, errors, signing),
        },
    ];
    let open = errors
        .fields()
        .next()
        .map_or(requested, PersonaTab::of_field);
    tabbed_form(
        &format!("/personas/{id}"),
        errors,
        &tabs,
        open.id(),
        "Save persona",
        TabsStyle::Page,
    )
}

/// Avatar, display name, harness and model of the persona.
fn heading(persona: &PersonaRecord) -> Markup {
    html! {
        div id="persona-heading" class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1" {
            (persona_avatar(Some(persona.id), &persona.display_name, AvatarSize::Card))
            h1 class="min-w-0 truncate text-2xl font-semibold" { (persona.display_name) }
            // On a phone the line sits below the name row.
            span class="text-[13px] text-muted max-sm:w-full max-sm:pl-13 sm:text-sm" { (harness_label(&persona.harness)) " · " (persona.model) }
        }
    }
}
