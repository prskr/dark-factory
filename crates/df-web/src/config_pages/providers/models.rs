//! The model list of the provider form, tab "Models" (iter3-opencode-harness design D4, D5).
//!
//! Rows are plain inputs named `model_id_<n>`, `model_api_<n>`, `model_context_<n>` and
//! `model_output_<n>`. "+ Add model" and the trash buttons post the form to
//! `/model-providers/refresh`, which renders it again with the row added or removed.

use std::collections::BTreeMap;

use df_core::model_provider::{ModelLimits, ProviderError, ProviderModel, WireApi};
use maud::{Markup, html};

use crate::components::forms::INPUT_CLASS;
use crate::components::{ButtonKind, FormErrors, ICON_BUTTON_CLASS, Icon, action_class, icon};

const HINT: &str = "Context and max output are in tokens: set both or neither. OpenCode uses them for context tracking and compaction.";

/// A model of a preset: ID, wire API, context window and max output tokens.
pub(super) struct PresetModel {
    pub(super) id: &'static str,
    pub(super) wire_api: WireApi,
    pub(super) context: i64,
    pub(super) output: i64,
}

/// One row of the form, as typed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ModelRow {
    pub(super) id: String,
    pub(super) wire_api: String,
    pub(super) context: String,
    pub(super) output: String,
}

impl ModelRow {
    pub(super) fn from_model(model: &ProviderModel) -> Self {
        Self {
            id: model.model_id.clone(),
            wire_api: model.wire_api.as_str().to_string(),
            context: model
                .limits
                .map(|l| l.context_window_tokens.to_string())
                .unwrap_or_default(),
            output: model
                .limits
                .map(|l| l.max_output_tokens.to_string())
                .unwrap_or_default(),
        }
    }

    pub(super) fn from_preset(model: &PresetModel) -> Self {
        Self {
            id: model.id.to_string(),
            wire_api: model.wire_api.as_str().to_string(),
            context: model.context.to_string(),
            output: model.output.to_string(),
        }
    }

    /// An empty row for `wire_api`.
    pub(super) fn blank(wire_api: WireApi) -> Self {
        Self {
            wire_api: wire_api.as_str().to_string(),
            ..Self::default()
        }
    }
}

/// The rows among the submitted `fields`, in the order of their numbers.
pub(super) fn rows_from_fields(fields: &BTreeMap<String, String>) -> Vec<ModelRow> {
    let mut numbers: Vec<usize> = fields
        .keys()
        .filter_map(|key| key.strip_prefix("model_id_")?.parse().ok())
        .collect();
    numbers.sort_unstable();
    let get = |name: &str, n: usize| {
        fields
            .get(&format!("{name}_{n}"))
            .cloned()
            .unwrap_or_default()
    };
    numbers
        .into_iter()
        .map(|n| ModelRow {
            id: get("model_id", n),
            wire_api: get("model_api", n),
            context: get("model_context", n),
            output: get("model_output", n),
        })
        .collect()
}

/// The field of the error under row `index`.
pub(super) fn row_field(index: usize) -> String {
    format!("model_{index}")
}

/// The models of the rows. A row with a problem adds an error under it and is left out.
pub(super) fn parse(rows: &[ModelRow], errors: &mut FormErrors) -> Vec<ProviderModel> {
    let mut models = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match parse_row(row) {
            Ok(model) => models.push(model),
            Err(message) => errors.add(row_field(index), message),
        }
    }
    models
}

fn parse_row(row: &ModelRow) -> Result<ProviderModel, &'static str> {
    let model_id = row.id.trim();
    if model_id.is_empty() {
        return Err("Model ID is required.");
    }
    let wire_api = row
        .wire_api
        .parse()
        .map_err(|_unknown| "Choose a wire API.")?;
    let (context, output) = (row.context.trim(), row.output.trim());
    let limits = match (context.is_empty(), output.is_empty()) {
        (true, true) => None,
        (false, false) => {
            let tokens = |text: &str| {
                text.parse::<i64>()
                    .map_err(|_invalid| "Limits are whole numbers of tokens.")
            };
            Some(ModelLimits {
                context_window_tokens: tokens(context)?,
                max_output_tokens: tokens(output)?,
            })
        }
        _ => return Err("Set both limits or neither."),
    };
    Ok(ProviderModel {
        model_id: model_id.to_string(),
        wire_api,
        limits,
    })
}

/// Whether `error` belongs to a model, and then its field and message: under the row it is
/// about, or for the whole list.
pub(super) fn core_error(error: &ProviderError, rows: &[ModelRow]) -> Option<(String, String)> {
    let position = |id: &str, last: bool| {
        let mut matching = rows.iter().enumerate().filter(|(_, r)| r.id.trim() == id);
        if last {
            matching.next_back()
        } else {
            matching.next()
        }
        .map(|(index, _)| index)
    };
    let (id, last, message) = match error {
        ProviderError::DuplicateModel(id) => (id, true, error.to_string()),
        ProviderError::ModelApiNotOffered { model, wire_api } => (
            model,
            false,
            format!(
                "{model} uses {}, which this provider does not offer (tab General).",
                wire_api.label()
            ),
        ),
        ProviderError::ModelLimitNotPositive(id) => (id, false, error.to_string()),
        ProviderError::EmptyModelId => return Some(("models".to_string(), error.to_string())),
        _ => return None,
    };
    position(id, last).map(|index| (row_field(index), message))
}

/// The widths of the Wire API and the limit columns and the class of the list of rows. The
/// settings page has room for a one-line row; the create modal keeps narrower columns and a
/// scrolling list.
#[derive(Clone, Copy)]
struct Layout {
    api_width: &'static str,
    limit_width: &'static str,
    list_class: &'static str,
}

impl Layout {
    const PAGE: Self = Self {
        api_width: "w-55",
        limit_width: "w-35",
        list_class: "space-y-2",
    };
    const DIALOG: Self = Self {
        api_width: "w-48",
        limit_width: "w-21",
        list_class: "max-h-96 space-y-2 overflow-y-auto",
    };
}

/// The panel of the tab. `wide` is the layout of the settings page, see [`Layout`].
pub(super) fn panel(rows: &[ModelRow], errors: &FormErrors, wide: bool) -> Markup {
    let layout = if wide { Layout::PAGE } else { Layout::DIALOG };
    html! {
        div class="flex items-start justify-between gap-4" {
            p class="text-sm text-muted" { "Personas of this provider pick one of these models. Each model uses one of the provider's wire APIs." }
            button type="button" hx-post="/model-providers/refresh" hx-vals=(r#"{"op":"add"}"#) hx-target="#form" hx-swap="outerHTML"
                class={(action_class(ButtonKind::Secondary)) " shrink-0"} { "+ Add model" }
        }
        @if let Some(error) = errors.get("models") { p class="text-sm text-danger" role="alert" { (error) } }
        div class="flex gap-2 text-xs text-muted" aria-hidden="true" {
            span class="min-w-0 flex-1" { "Model ID" } span class={(layout.api_width) " shrink-0"} { "Wire API" }
            span class={(layout.limit_width) " shrink-0"} { "Context" } span class={(layout.limit_width) " shrink-0"} { "Max output" } span class="w-11 shrink-0 lg:w-9" {}
        }
        div class=(layout.list_class) {
            @for (index, row) in rows.iter().enumerate() { (row_markup(index, row, errors, layout)) }
        }
        p class="text-xs text-faint" { (HINT) }
    }
}

fn row_markup(index: usize, row: &ModelRow, errors: &FormErrors, layout: Layout) -> Markup {
    let error = errors.get(&row_field(index));
    let invalid = error.map(|_| "true");
    let selected = row.wire_api.parse().unwrap_or(WireApi::ALL[0]);
    // The width sits on a wrapper: INPUT_CLASS carries `w-full`, which would win over a width on
    // the field itself and stretch every field to the width of the row.
    let number = |name: &str, label: &str, value: &str| {
        html! {
            div class={(layout.limit_width) " shrink-0"} {
                input name={(name) "_" (index)} type="text" inputmode="numeric" value=(value) aria-label=(label) aria-invalid=[invalid]
                    class={(INPUT_CLASS) " font-mono"};
            }
        }
    };
    html! {
        div class="space-y-1" {
            div class="flex items-center gap-2" {
                div class="min-w-0 flex-1" {
                    input name={"model_id_" (index)} type="text" value=(row.id) aria-label="Model ID" aria-invalid=[invalid]
                        class={(INPUT_CLASS) " font-mono"};
                }
                div class={(layout.api_width) " shrink-0"} {
                    select name={"model_api_" (index)} aria-label="Wire API" aria-invalid=[invalid] class=(INPUT_CLASS) {
                        @for api in WireApi::ALL { option value=(api.as_str()) selected[api == selected] { (api.label()) } }
                    }
                }
                (number("model_context", "Context window in tokens", &row.context))
                (number("model_output", "Max output tokens", &row.output))
                button type="button" hx-post="/model-providers/refresh" hx-vals=(format!(r#"{{"op":"remove:{index}"}}"#))
                    hx-target="#form" hx-swap="outerHTML" aria-label={"Remove model " (row.id)} title="Remove" class=(ICON_BUTTON_CLASS) {
                    (icon(Icon::Trash))
                }
            }
            @if let Some(error) = error { p class="text-sm text-danger" role="alert" { (error) } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, api: &str, context: &str, output: &str) -> ModelRow {
        ModelRow {
            id: id.into(),
            wire_api: api.into(),
            context: context.into(),
            output: output.into(),
        }
    }

    #[test]
    fn rows_are_read_in_the_order_of_their_numbers() {
        let fields: BTreeMap<String, String> = [
            ("model_id_10", "b"),
            ("model_id_2", "a"),
            ("model_api_2", "openai-chat"),
            ("model_context_2", "5"),
            ("name", "x"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let rows = rows_from_fields(&fields);

        assert_eq!(rows[0], row("a", "openai-chat", "5", ""));
        assert_eq!(rows[1].id, "b");
    }

    #[test]
    fn a_row_with_one_limit_gets_the_both_or_neither_error_under_it() {
        let mut errors = FormErrors::default();
        let models = parse(
            &[
                row("ok", "openai-chat", "", ""),
                row("one", "openai-chat", "1000", ""),
            ],
            &mut errors,
        );
        assert_eq!(models.len(), 1);
        assert_eq!(errors.get("model_1"), Some("Set both limits or neither."));
    }

    #[test]
    fn a_row_needs_an_id_a_known_wire_api_and_numeric_limits() {
        let mut errors = FormErrors::default();
        parse(
            &[
                row(" ", "openai-chat", "", ""),
                row("a", "grpc", "", ""),
                row("b", "openai-chat", "x", "1"),
            ],
            &mut errors,
        );
        assert_eq!(errors.get("model_0"), Some("Model ID is required."));
        assert_eq!(errors.get("model_1"), Some("Choose a wire API."));
        assert_eq!(
            errors.get("model_2"),
            Some("Limits are whole numbers of tokens.")
        );
    }

    #[test]
    fn a_duplicate_is_reported_under_its_second_row() {
        let rows = [
            row("glm-5.3", "openai-chat", "", ""),
            row("glm-5.3", "anthropic-messages", "", ""),
        ];
        let (field, message) =
            core_error(&ProviderError::DuplicateModel("glm-5.3".into()), &rows).unwrap();
        assert_eq!(
            (field.as_str(), message.as_str()),
            ("model_1", "Model glm-5.3 is listed twice.")
        );
    }
}
