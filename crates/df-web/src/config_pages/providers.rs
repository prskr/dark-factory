//! Model provider pages (`specs/model-providers/spec.md`): the upstream model services the
//! gateway forwards to, with their wire APIs, credential scheme and extra headers.

use axum::extract::{Form, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Router};
use df_core::CredentialRef;
use df_core::harness;
use df_core::model_provider::{
    CredentialScheme, HeaderTemplate, ProviderApi, ProviderConfig, ProviderError, WireApi,
};
use df_db::DbError;
use df_db::catalog::Actor;
use df_db::model_providers::{ModelProviderInput, ModelProviderRecord};
use maud::{Markup, html};
use serde::Deserialize;
use uuid::Uuid;

use super::{
    conflict, create_button, edit_button, form_footer, form_grid, full_row, list_header, list_page,
    modal, saved,
};
use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::invalid as invalid_attr;
use crate::components::forms::{INPUT_CLASS, described_by, field, field_with_markup_hint};
use crate::components::{
    Cell, FormErrors, ICON_BUTTON_CLASS, Icon, data_table, empty_panel, form_error, icon,
    select_field, td, text_field, th, tr,
};
use crate::layout::{Nav, Shell};
use crate::responses::{internal_error, invalid, not_found};

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/model-providers", get(list).post(create))
        .route("/model-providers/new", get(new))
        .route("/model-providers/preset", get(preset_form))
        .route("/model-providers/{id}", post(update).merge(delete(remove)))
        .route("/model-providers/{id}/edit", get(edit))
}

/// The compatible providers for the persona form: id and name.
pub(super) fn compatible_options(
    providers: &[ModelProviderRecord],
    harness_name: &str,
) -> Vec<(String, String)> {
    providers
        .iter()
        .filter(|provider| {
            harness::check_compatible(harness_name, &provider.wire_apis(), provider.input.scheme)
                .is_ok()
        })
        .map(|provider| (provider.id.to_string(), provider.input.name.clone()))
        .collect()
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct ProviderForm {
    name: String,
    credential_ref_id: String,
    scheme: String,
    credential_header: String,
    credential_prefix: String,
    /// One `Name: template` per line.
    headers: String,
    api_anthropic_messages: String,
    url_anthropic_messages: String,
    api_openai_chat: String,
    url_openai_chat: String,
    api_openai_responses: String,
    url_openai_responses: String,
}

/// A provider configuration the form can be prefilled with. Presets are not stored.
struct Preset {
    id: &'static str,
    label: &'static str,
    apis: &'static [(WireApi, &'static str)],
    scheme: CredentialScheme,
    headers: &'static [(&'static str, &'static str)],
}

/// The text an example token shows next to the prefix. A fixed text, never derived from the
/// credential: the control plane does not read the Secret for this form.
const EXAMPLE_TOKEN: &str = "sk-…example…4f7a";

/// The hint of the extra headers while the scheme allows them.
const HEADERS_HINT: &str = "One “Name: value” per line. Values are templates over task_id, task_key, run_id, project_key and persona_id with the sha256 filter, for example {{ task_id | sha256 }}.";

const PRESETS: [Preset; 4] = [
    Preset {
        id: "anthropic",
        label: "Anthropic",
        apis: &[(WireApi::AnthropicMessages, "https://api.anthropic.com")],
        scheme: CredentialScheme::XApiKey,
        headers: &[],
    },
    Preset {
        id: "claude-subscription",
        label: "Claude subscription",
        apis: &[(WireApi::AnthropicMessages, "https://api.anthropic.com")],
        scheme: CredentialScheme::ClaudeSubscription,
        headers: &[],
    },
    Preset {
        id: "openai",
        label: "OpenAI",
        apis: &[
            (WireApi::OpenAiChat, "https://api.openai.com"),
            (WireApi::OpenAiResponses, "https://api.openai.com"),
        ],
        scheme: CredentialScheme::Bearer,
        headers: &[],
    },
    Preset {
        id: "opencode-go",
        label: "OpenCode Go",
        apis: &[
            (
                WireApi::OpenAiChat,
                "https://opencode.ai/inference/go/openai",
            ),
            (
                WireApi::OpenAiResponses,
                "https://opencode.ai/inference/go/openai",
            ),
            (
                WireApi::AnthropicMessages,
                "https://opencode.ai/inference/go/anthropic",
            ),
        ],
        scheme: CredentialScheme::Bearer,
        headers: &[("x-opencode-session", "{{ task_id | sha256 }}")],
    },
];

// ---------------------------------------------------------------------------------------------
// Form
// ---------------------------------------------------------------------------------------------

/// The fields of a wire API in the form: its checkbox and its URL input.
fn api_fields(api: WireApi) -> (&'static str, &'static str) {
    match api {
        WireApi::AnthropicMessages => ("api_anthropic_messages", "url_anthropic_messages"),
        WireApi::OpenAiChat => ("api_openai_chat", "url_openai_chat"),
        WireApi::OpenAiResponses => ("api_openai_responses", "url_openai_responses"),
    }
}

impl ProviderForm {
    /// The checkbox and URL value of `api`.
    fn api_values(&self, api: WireApi) -> (bool, &str) {
        match api {
            WireApi::AnthropicMessages => (
                !self.api_anthropic_messages.is_empty(),
                &self.url_anthropic_messages,
            ),
            WireApi::OpenAiChat => (!self.api_openai_chat.is_empty(), &self.url_openai_chat),
            WireApi::OpenAiResponses => (
                !self.api_openai_responses.is_empty(),
                &self.url_openai_responses,
            ),
        }
    }

    fn set_api(&mut self, api: WireApi, url: &str) {
        let (checkbox, field) = match api {
            WireApi::AnthropicMessages => (
                &mut self.api_anthropic_messages,
                &mut self.url_anthropic_messages,
            ),
            WireApi::OpenAiChat => (&mut self.api_openai_chat, &mut self.url_openai_chat),
            WireApi::OpenAiResponses => (
                &mut self.api_openai_responses,
                &mut self.url_openai_responses,
            ),
        };
        "on".clone_into(checkbox);
        url.clone_into(field);
    }

    fn from_preset(preset: &Preset, name: &str) -> Self {
        let mut form = Self {
            name: name.to_string(),
            scheme: preset.scheme.as_str().to_string(),
            headers: preset
                .headers
                .iter()
                .map(|(name, template)| format!("{name}: {template}"))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Self::default()
        };
        for (api, url) in preset.apis {
            form.set_api(*api, url);
        }
        form
    }

    fn from_record(record: &ModelProviderRecord) -> Self {
        let input = &record.input;
        let mut form = Self {
            name: input.name.clone(),
            credential_ref_id: input.credential_ref_id.to_string(),
            scheme: input.scheme.as_str().to_string(),
            credential_header: input.credential_header.clone().unwrap_or_default(),
            credential_prefix: input.credential_prefix.clone().unwrap_or_default(),
            headers: input
                .headers
                .iter()
                .map(|header| format!("{}: {}", header.name, header.template))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Self::default()
        };
        for api in &input.apis {
            form.set_api(api.wire_api, &api.upstream_url);
        }
        form
    }
}

/// Parses the headers textarea: one `Name: template` per line, blank lines skipped.
fn parse_headers(text: &str) -> Result<Vec<HeaderTemplate>, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, template) = line
                .split_once(':')
                .ok_or_else(|| format!("`{line}` must look like `Name: value`."))?;
            Ok(HeaderTemplate {
                name: name.trim().to_string(),
                template: template.trim().to_string(),
            })
        })
        .collect()
}

/// The form field a provider error belongs to.
fn error_field(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::NoApi | ProviderError::DuplicateApi(_) => "apis",
        ProviderError::InvalidUrl(api) => api_fields(*api).1,
        ProviderError::SubscriptionNeedsAnthropic => "scheme",
        ProviderError::CredentialHeaderMissing | ProviderError::CredentialHeader(_) => {
            "credential_header"
        }
        ProviderError::Header { .. }
        | ProviderError::DuplicateHeader(_)
        | ProviderError::SubscriptionNoHeaders => "headers",
    }
}

fn validate(form: &ProviderForm, credentials: &[Uuid]) -> Result<ModelProviderInput, FormErrors> {
    let mut errors = FormErrors::default();
    let name = form.name.trim().to_string();
    if name.is_empty() {
        errors.add("name", "Name is required.");
    }
    let credential_ref_id = match Uuid::parse_str(&form.credential_ref_id) {
        Ok(id) if credentials.contains(&id) => id,
        _ => {
            errors.add("credential_ref_id", "Choose a credential reference.");
            Uuid::nil()
        }
    };
    let scheme = form.scheme.parse::<CredentialScheme>().unwrap_or_else(|_| {
        errors.add("scheme", "Choose a credential scheme.");
        CredentialScheme::Bearer
    });
    let apis: Vec<ProviderApi> = WireApi::ALL
        .into_iter()
        .filter_map(|api| {
            let (checked, url) = form.api_values(api);
            checked.then(|| ProviderApi {
                wire_api: api,
                upstream_url: url.trim().to_string(),
            })
        })
        .collect();
    let headers = parse_headers(&form.headers).unwrap_or_else(|message| {
        errors.add("headers", message);
        Vec::new()
    });
    let uses_header = scheme == CredentialScheme::Header;
    let optional = |value: &str, keep: bool| {
        Some(value.trim().to_string()).filter(|value| keep && !value.is_empty())
    };
    let config = ProviderConfig {
        apis,
        scheme,
        credential_header: optional(&form.credential_header, uses_header),
        headers,
    };
    if errors.is_empty()
        && let Err(error) = config.validate()
    {
        errors.add(error_field(&error), error.to_string());
    }
    errors.into_result(ModelProviderInput {
        name,
        credential_ref_id,
        scheme,
        credential_header: config.credential_header,
        // Spaces count: `Token ` before the value.
        credential_prefix: Some(form.credential_prefix.clone())
            .filter(|prefix| uses_header && !prefix.is_empty()),
        headers: config.headers,
        apis: config.apis,
    })
}

fn credential_options(credentials: &[CredentialRef]) -> Vec<(String, String)> {
    credentials
        .iter()
        .map(|c| {
            (
                c.id.to_string(),
                format!("{} ({}/{})", c.name, c.secret_name, c.secret_key),
            )
        })
        .collect()
}

fn api_row(api: WireApi, values: &ProviderForm, errors: &FormErrors) -> Markup {
    let (checkbox, url) = api_fields(api);
    let (checked, value) = values.api_values(api);
    html! {
        div class="space-y-1" {
            div class="flex flex-wrap items-center gap-3" {
                label class="inline-flex min-h-11 w-56 shrink-0 cursor-pointer items-center gap-2 text-sm" for=(checkbox) {
                    input id=(checkbox) name=(checkbox) type="checkbox" value="on" checked[checked] class="size-4 accent-accent-text";
                    (api.label())
                }
                input id=(url) name=(url) type="text" value=(value) placeholder="https://api.example.org"
                    aria-label={(api.label()) " upstream URL"} aria-invalid=[errors.get(url).map(|_| "true")]
                    class={(INPUT_CLASS) " min-w-0 flex-1 font-mono"};
            }
            @if let Some(error) = errors.get(url) { p class="text-sm text-danger" role="alert" { (error) } }
        }
    }
}

fn provider_form(
    action: &str,
    values: &ProviderForm,
    errors: &FormErrors,
    credentials: &[CredentialRef],
    create: bool,
) -> Markup {
    let submit = if create {
        "Create model provider"
    } else {
        "Save model provider"
    };
    html! {
        form id="form" class="space-y-4" x-data=(scheme_state(values)) hx-post=(action) hx-target="#form" hx-swap="outerHTML" {
            (form_error(errors))
            (form_grid(html! {
                (text_field("Name", "name", &values.name, errors, None))
                @if create {
                    (preset_select())
                }
                (full_row(html! {
                    fieldset class="space-y-1" {
                        legend class="block text-sm font-medium text-muted" { "Wire APIs and upstream URLs" }
                        @for api in WireApi::ALL { (api_row(api, values, errors)) }
                        @if let Some(error) = errors.get("apis") { p class="text-sm text-danger" role="alert" { (error) } }
                        p class="text-xs text-faint" { "Dark Factory passes each API through unchanged. Upstream URLs use HTTPS." }
                    }
                }))
                (select_field("Credential reference", "credential_ref_id", &credential_options(credentials),
                    &values.credential_ref_id, Some("Choose…"), errors))
                (scheme_select(values, errors))
                (scheme_fields(values, errors))
            }))
            (form_footer(submit))
        }
    }
}

/// The scheme select. A change is handled in the browser, see [`scheme_state`].
fn scheme_select(values: &ProviderForm, errors: &FormErrors) -> Markup {
    let selected = effective_scheme(values);
    field(
        "Credential scheme",
        "scheme",
        errors,
        None,
        html! {
            select id="scheme" name="scheme" class=(INPUT_CLASS) x-model="scheme"
                aria-invalid=[invalid_attr("scheme", errors)] aria-describedby=[described_by("scheme", false, errors)] {
                @for scheme in CredentialScheme::ALL {
                    option value=(scheme.as_str()) selected[scheme == selected] { (scheme.label()) }
                }
            }
        },
    )
}

/// The scheme of the form; the first choice while none is chosen yet (what a browser selects).
fn effective_scheme(values: &ProviderForm) -> CredentialScheme {
    values.scheme.parse().unwrap_or(CredentialScheme::ALL[0])
}

/// The Alpine state of the form (`x-data`, plain data): the chosen scheme, what was typed in
/// Header, Value and Extra headers, and the header and prefix the schemes that fix them send.
/// Changing the scheme only enables, disables and previews, so typed values survive any sequence
/// of changes. Disabled controls are not submitted; the server stores what the scheme uses.
fn scheme_state(values: &ProviderForm) -> String {
    let fixed: serde_json::Map<String, serde_json::Value> = CredentialScheme::ALL
        .into_iter()
        .filter_map(|scheme| {
            let prefix = scheme.fixed_prefix()?;
            let header = scheme.header_name(None)?;
            Some((
                scheme.as_str().to_string(),
                serde_json::json!({ "header": header, "prefix": prefix }),
            ))
        })
        .collect();
    serde_json::json!({
        "scheme": effective_scheme(values).as_str(),
        "header": values.credential_header,
        "prefix": values.credential_prefix,
        "headers": values.headers,
        "fixed": fixed,
    })
    .to_string()
}

/// A hint with two texts; the Alpine condition `when` picks the first, `shown_first` is what the
/// server renders before Alpine runs.
fn switched_hint(when: &str, first: &str, second: &str, shown_first: bool) -> Markup {
    let hidden = "display: none";
    html! {
        span x-show=(when) style=[(!shown_first).then_some(hidden)] { (first) }
        span x-show={"!(" (when) ")"} style=[shown_first.then_some(hidden)] { (second) }
    }
}

/// Header, Value (prefix and an example token) and Extra headers, which depend on the scheme.
/// The same controls render for every scheme, disabled where the scheme fixes or forbids them,
/// so the dialog keeps its size when the scheme changes.
fn scheme_fields(values: &ProviderForm, errors: &FormErrors) -> Markup {
    let scheme = effective_scheme(values);
    let custom = scheme == CredentialScheme::Header;
    let subscription = scheme == CredentialScheme::ClaudeSubscription;
    let header = scheme
        .header_name(None)
        .unwrap_or(&values.credential_header);
    let prefix = scheme.fixed_prefix().unwrap_or(&values.credential_prefix);
    // Alpine conditions on the scheme selected in the form.
    let is_custom = format!("scheme === '{}'", CredentialScheme::Header.as_str());
    let not_custom = format!("scheme !== '{}'", CredentialScheme::Header.as_str());
    let is_subscription = format!(
        "scheme === '{}'",
        CredentialScheme::ClaudeSubscription.as_str()
    );
    let value_box = "flex min-h-11 items-stretch overflow-hidden rounded-lg border lg:min-h-10";
    html! {
        div class="contents" {
            (field_with_markup_hint("Header", "credential_header", errors,
                Some(switched_hint(&is_custom,
                    "Any header except hop-by-hop headers, Host and Content-Length.", "Set by the scheme.", custom)),
                html! {
                    input id="credential_header" name="credential_header" type="text" value=(header) disabled[!custom]
                        x-bind:value={(is_custom) " ? header : fixed[scheme].header"}
                        "x-on:input"="header = $event.target.value" "x-bind:disabled"=(not_custom)
                        aria-invalid=[invalid_attr("credential_header", errors)]
                        aria-describedby=[described_by("credential_header", true, errors)]
                        class={(INPUT_CLASS) " font-mono"};
                }))
            (field("Value", "credential_prefix", errors,
                Some("Prefix, then the secret. The token is an example."),
                html! {
                    div class=(value_box)
                        "x-bind:class"={"{ 'border-line-strong bg-panel': " (is_custom) ", 'border-line bg-sunk': " (not_custom) " }"} {
                        input id="credential_prefix" name="credential_prefix" type="text" value=(prefix) disabled[!custom]
                            x-bind:value={(is_custom) " ? prefix : fixed[scheme].prefix"}
                            "x-on:input"="prefix = $event.target.value" "x-bind:disabled"=(not_custom)
                            aria-invalid=[invalid_attr("credential_prefix", errors)]
                            aria-describedby=[described_by("credential_prefix", true, errors)]
                            class="w-24 shrink-0 border-0 border-r border-line bg-transparent px-3 font-mono text-base text-ink disabled:text-faint";
                        span id="credential_example" class="min-w-0 flex-1 self-center truncate px-3 font-mono text-base text-faint" { (EXAMPLE_TOKEN) }
                    }
                }))
            (full_row(field_with_markup_hint("Extra headers", "headers", errors,
                Some(switched_hint(&is_subscription,
                    "Not available with the Claude subscription scheme.", HEADERS_HINT, subscription)),
                html! {
                    textarea id="headers" name="headers" rows="3" disabled[subscription] x-model="headers"
                        "x-bind:disabled"=(is_subscription)
                        aria-invalid=[invalid_attr("headers", errors)] aria-describedby=[described_by("headers", true, errors)]
                        class={(INPUT_CLASS) " font-mono"} { (values.headers) }
                })))
        }
    }
}

/// Picking a preset reloads the form with its values; the name typed so far stays.
fn preset_select() -> Markup {
    let options: Vec<(String, String)> = PRESETS
        .iter()
        .map(|preset| (preset.id.to_string(), preset.label.to_string()))
        .collect();
    html! {
        div class="space-y-1" {
            label for="preset" class="block text-sm font-medium text-muted" { "Preset" }
            select id="preset" name="preset" class=(INPUT_CLASS)
                hx-get="/model-providers/preset" hx-trigger="change" hx-target="#form" hx-swap="outerHTML" hx-include="#name" {
                option value="" { "Start from a preset…" }
                @for (id, label) in &options { option value=(id) { (label) } }
            }
            p class="text-xs text-faint" { "Presets only prefill the form; the provider stores the values you save." }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------------------------

struct ListEntry {
    record: ModelProviderRecord,
    personas: Vec<String>,
}

fn region(entries: &[ListEntry], credentials: &[CredentialRef], notice: Option<&str>) -> Markup {
    html! {
        div id="config-list" {
            (list_header("Model providers", entries.len(), create_button("/model-providers/new", "New model provider"), None))
            @if let Some(notice) = notice {
                p class="mb-4 rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger" role="alert" { (notice) }
            }
            @if entries.is_empty() {
                (empty_panel(
                    "No model providers yet",
                    "A model provider is an upstream model service with the credential Dark Factory uses for it. Personas use one; the gateway adds the credential, so no sandbox ever holds it.",
                    create_button("/model-providers/new", "New model provider"),
                ))
            } @else {
                (data_table(
                    html! {
                        (th(Cell::First, html! { "Name" })) (th(Cell::Text, html! { "Wire APIs" }))
                        (th(Cell::Text, html! { "Scheme" })) (th(Cell::Mono, html! { "Credential" }))
                        (th(Cell::Text, html! { "Used by" }))
                        (th(Cell::Actions, html! { span class="sr-only" { "Actions" } }))
                    },
                    html! {
                        @for entry in entries { (row(entry, credentials)) }
                    },
                ))
            }
        }
    }
}

fn row(entry: &ListEntry, credentials: &[CredentialRef]) -> Markup {
    let input = &entry.record.input;
    let id = entry.record.id;
    let credential = credentials
        .iter()
        .find(|c| c.id.as_uuid() == input.credential_ref_id);
    html! {
        (tr(html! {
            (td(Cell::First, html! { (input.name) }))
            (td(Cell::Text, html! {
                (input.apis.iter().map(|api| api.wire_api.label()).collect::<Vec<_>>().join(", "))
            }))
            (td(Cell::Text, html! { (input.scheme.label()) }))
            (td(Cell::Mono, html! {
                @if let Some(c) = credential { (c.name) " (" (c.secret_name) "/" (c.secret_key) ")" } @else { "missing" }
            }))
            (td(Cell::Text, html! {
                @if entry.personas.is_empty() { span class="text-faint" { "no persona" } }
                @else { (entry.personas.join(", ")) }
            }))
            (td(Cell::Actions, html! {
                div class="flex items-center gap-1" {
                    (edit_button(&format!("/model-providers/{id}/edit"), &format!("Edit model provider {}", input.name)))
                    button type="button" hx-delete={"/model-providers/" (id)} hx-target="#config-list" hx-swap="outerHTML"
                        hx-confirm={"Delete model provider " (input.name) "?"}
                        aria-label={"Delete model provider " (input.name)} title="Delete" class=(ICON_BUTTON_CLASS) {
                        (icon(Icon::Trash))
                    }
                }
            }))
        }))
    }
}

async fn entries(state: &WebState) -> Result<(Vec<ListEntry>, Vec<CredentialRef>), DbError> {
    let (records, credentials) = tokio::try_join!(
        df_db::model_providers::list_model_providers(&state.pool),
        df_db::catalog::list_credential_refs(&state.pool)
    )?;
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let personas = df_db::model_providers::personas_using(&state.pool, record.id).await?;
        entries.push(ListEntry { record, personas });
    }
    Ok((entries, credentials))
}

async fn list_markup(state: &WebState, notice: Option<&str>) -> Result<Markup, Box<Response>> {
    let (entries, credentials) = entries(state)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(region(&entries, &credentials, notice))
}

async fn list(State(state): State<WebState>, shell: Shell) -> Response {
    match list_markup(&state, None).await {
        Ok(markup) => list_page("Model providers", Nav::ModelProviders, &shell, markup),
        Err(response) => *response,
    }
}

// ---------------------------------------------------------------------------------------------
// Create, edit, delete
// ---------------------------------------------------------------------------------------------

async fn new(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/model-providers").into_response();
    }
    match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(credentials) => modal(
            "New model provider",
            provider_form(
                "/model-providers",
                &ProviderForm::default(),
                &FormErrors::default(),
                &credentials,
                true,
            ),
        ),
        Err(e) => internal_error(&e),
    }
}

#[derive(Deserialize)]
struct PresetQuery {
    preset: String,
    #[serde(default)]
    name: String,
}

/// The create form prefilled with a preset (the fragment that replaces `#form`).
async fn preset_form(State(state): State<WebState>, Query(query): Query<PresetQuery>) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(credentials) => credentials,
        Err(e) => return internal_error(&e),
    };
    let values = PRESETS
        .iter()
        .find(|preset| preset.id == query.preset)
        .map_or_else(
            || ProviderForm {
                name: query.name.clone(),
                ..ProviderForm::default()
            },
            |preset| ProviderForm::from_preset(preset, &query.name),
        );
    provider_form(
        "/model-providers",
        &values,
        &FormErrors::default(),
        &credentials,
        true,
    )
    .into_response()
}

async fn edit(State(state): State<WebState>, headers: HeaderMap, Path(id): Path<Uuid>) -> Response {
    if !crate::auth::is_htmx(&headers) {
        return Redirect::to("/model-providers").into_response();
    }
    let (record, credentials) = match tokio::try_join!(
        df_db::model_providers::get_model_provider(&state.pool, id),
        df_db::catalog::list_credential_refs(&state.pool)
    ) {
        Ok((Some(record), credentials)) => (record, credentials),
        Ok((None, _)) => return not_found("The model provider"),
        Err(e) => return internal_error(&e),
    };
    modal(
        "Edit model provider",
        provider_form(
            &format!("/model-providers/{id}"),
            &ProviderForm::from_record(&record),
            &FormErrors::default(),
            &credentials,
            false,
        ),
    )
}

async fn create(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    form: Form<ProviderForm>,
) -> Response {
    save(state, user, None, form.0).await
}

async fn update(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<Uuid>,
    form: Form<ProviderForm>,
) -> Response {
    save(state, user, Some(id), form.0).await
}

/// Errors for personas that could no longer use the provider with `input`.
async fn persona_conflicts(
    state: &WebState,
    id: Uuid,
    input: &ModelProviderInput,
) -> Result<Vec<String>, DbError> {
    let wire_apis: Vec<WireApi> = input.apis.iter().map(|api| api.wire_api).collect();
    let mut problems = Vec::new();
    for persona in df_db::catalog::list_personas(&state.pool)
        .await?
        .into_iter()
        .filter(|persona| persona.model_provider_id == id)
    {
        if let Err(error) = harness::check_compatible(&persona.harness, &wire_apis, input.scheme) {
            problems.push(format!("Persona {}: {error}", persona.name));
        }
    }
    Ok(problems)
}

async fn save(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    id: Option<Uuid>,
    form: ProviderForm,
) -> Response {
    let credentials = match df_db::catalog::list_credential_refs(&state.pool).await {
        Ok(credentials) => credentials,
        Err(e) => return internal_error(&e),
    };
    let known: Vec<Uuid> = credentials.iter().map(|c| c.id.as_uuid()).collect();
    let errors = match validate(&form, &known) {
        Ok(input) => match store(&state, &user, id, &input).await {
            Ok(Some(errors)) => errors,
            Ok(None) => {
                return match list_markup(&state, None).await {
                    Ok(markup) => saved(markup),
                    Err(response) => *response,
                };
            }
            Err(response) => return *response,
        },
        Err(errors) => errors,
    };
    let action = id.map_or_else(
        || "/model-providers".to_string(),
        |id| format!("/model-providers/{id}"),
    );
    invalid(provider_form(
        &action,
        &form,
        &errors,
        &credentials,
        id.is_none(),
    ))
}

/// Stores a valid provider: `Ok(None)` when saved, form errors when a rule of the database or of
/// the personas that use it refuses it.
async fn store(
    state: &WebState,
    user: &CurrentUser,
    id: Option<Uuid>,
    input: &ModelProviderInput,
) -> Result<Option<FormErrors>, Box<Response>> {
    let mut errors = FormErrors::default();
    if let Some(id) = id {
        let problems = persona_conflicts(state, id, input)
            .await
            .map_err(|e| Box::new(internal_error(&e)))?;
        if !problems.is_empty() {
            errors.add("", problems.join(" "));
            return Ok(Some(errors));
        }
    }
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    match df_db::model_providers::save_model_provider(&state.pool, actor, id, input).await {
        Ok(_) => Ok(None),
        Err(DbError::NotFound(_)) => Err(Box::new(not_found("The model provider"))),
        Err(e) => match conflict(&mut errors, "name", e) {
            Some(response) => Err(Box::new(response)),
            None => Ok(Some(errors)),
        },
    }
}

async fn remove(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<Uuid>,
) -> Response {
    let principal = user.principal();
    let actor = Actor {
        principal: &principal,
        roles: &user.roles,
    };
    let notice = match df_db::model_providers::delete_model_provider(&state.pool, actor, id).await {
        Ok(()) => None,
        Err(DbError::Conflict(message)) => Some(message),
        Err(DbError::NotFound(_)) => return not_found("The model provider"),
        Err(e) => return internal_error(&e),
    };
    match list_markup(&state, notice.as_deref()).await {
        Ok(markup) => markup.into_response(),
        Err(response) => *response,
    }
}

#[cfg(test)]
mod tests;
